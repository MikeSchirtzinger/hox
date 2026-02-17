//! JJ-lib direct integration backend (feature-gated)
//!
//! Strategy: jj-lib for reads (hot path), CLI for writes (preserves oplog contract).
//! Every jj-lib read wraps in try-then-fallback-to-CLI.

#[cfg(feature = "jj-lib-integration")]
mod inner {
    use async_trait::async_trait;
    use hox_core::Result;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tracing::{debug, instrument, warn};

    use crate::command::{JjCommand, JjExecutor, JjOutput};

    use jj_lib::config::StackedConfig;
    use jj_lib::object_id::ObjectId;
    use jj_lib::repo::{ReadonlyRepo, Repo, StoreFactories};
    use jj_lib::revset::{
        self, RevsetAliasesMap, RevsetDiagnostics, RevsetExtensions, RevsetIteratorExt,
        RevsetParseContext, SymbolResolver,
    };
    use jj_lib::settings::UserSettings;
    use jj_lib::time_util::DatePatternContext;
    use jj_lib::workspace::{self, Workspace};

    /// Cached read-only repository handle
    struct RepoHandle {
        repo: Arc<ReadonlyRepo>,
        op_id: String,
    }

    /// Classification of jj operations
    #[derive(Debug, PartialEq)]
    enum OpKind {
        Read,
        Write,
    }

    /// JJ executor using jj-lib for hot-path read operations.
    ///
    /// Policy: jj-lib for reads, CLI for writes. One CLI invocation = one oplog entry.
    #[derive(Clone)]
    pub struct JjLibExecutor {
        repo_root: PathBuf,
        fallback: JjCommand,
        handle: Arc<Mutex<Option<RepoHandle>>>,
    }

    impl JjLibExecutor {
        /// Create a new JjLibExecutor for the given repository
        pub fn new(repo_root: impl Into<PathBuf>) -> Self {
            let repo_root = repo_root.into();
            Self {
                fallback: JjCommand::new(repo_root.clone()),
                repo_root,
                handle: Arc::new(Mutex::new(None)),
            }
        }

        /// Auto-detect repository root from current directory
        pub async fn detect() -> Result<Self> {
            let cmd = JjCommand::detect().await?;
            Ok(Self::new(cmd.repo_root().clone()))
        }

        /// Classify args as Read or Write
        fn classify(args: &[&str]) -> OpKind {
            match args.first().copied() {
                Some("log" | "op" | "bookmark" | "status" | "show" | "evolog") => {
                    // "bookmark list" is read, "bookmark create/set/delete" is write
                    if args.first() == Some(&"bookmark") {
                        match args.get(1).copied() {
                            Some("list") => OpKind::Read,
                            _ => OpKind::Write,
                        }
                    // "op log" is read, other op subcommands are write
                    } else if args.first() == Some(&"op") {
                        match args.get(1).copied() {
                            Some("log") => OpKind::Read,
                            _ => OpKind::Write,
                        }
                    } else {
                        OpKind::Read
                    }
                }
                _ => OpKind::Write,
            }
        }

        /// Invalidate the cached repo handle (call after writes)
        fn invalidate_cache(&self) {
            if let Ok(mut guard) = self.handle.lock() {
                *guard = None;
            }
        }

        /// Get or refresh the repo handle. Returns None if jj-lib init fails.
        fn get_repo(&self) -> std::result::Result<Arc<ReadonlyRepo>, String> {
            let mut guard = self.handle.lock().map_err(|e| format!("lock poisoned: {e}"))?;

            // If cached, check freshness by comparing op_id
            if let Some(ref handle) = *guard {
                // Return cached repo — caller invalidates on writes
                return Ok(Arc::clone(&handle.repo));
            }

            // Load workspace from scratch
            let config = StackedConfig::empty();
            let settings = UserSettings::from_config(config)
                .map_err(|e| format!("UserSettings init: {e}"))?;
            let store_factories = StoreFactories::default();
            let wc_factories = workspace::default_working_copy_factories();

            let ws = Workspace::load(&settings, &self.repo_root, &store_factories, &wc_factories)
                .map_err(|e| format!("Workspace::load: {e}"))?;

            let repo = ws
                .repo_loader()
                .load_at_head()
                .map_err(|e| format!("load_at_head: {e}"))?;

            let op_id = repo.operation().id().hex();
            *guard = Some(RepoHandle {
                repo: Arc::clone(&repo),
                op_id,
            });

            Ok(repo)
        }

        /// Try to handle a read via jj-lib
        async fn try_jjlib_read(&self, args: &[&str]) -> std::result::Result<JjOutput, String> {
            match args.first().copied() {
                Some("log") => self.jjlib_log(args),
                Some("op") => self.jjlib_op(args),
                Some("bookmark") => self.jjlib_bookmark_list(args),
                Some("status") => self.jjlib_status(),
                Some("show") => self.jjlib_show(args),
                Some("evolog") => self.jjlib_evolog(args),
                _ => Err(format!("unhandled read: {args:?}")),
            }
        }

        /// jj log -r {revset} -T {template} [--no-graph]
        fn jjlib_log(&self, args: &[&str]) -> std::result::Result<JjOutput, String> {
            let repo = self.get_repo()?;

            // Parse -r and -T from args
            let revset_str = Self::extract_arg(args, "-r").unwrap_or("@");
            let template_str = Self::extract_arg(args, "-T");

            // Evaluate revset
            let commits = self.eval_revset(&repo, revset_str)?;

            // Format output based on template
            let mut output = String::new();
            for commit in &commits {
                let line = match template_str {
                    Some(t) if t.contains("description") => commit.description().to_string(),
                    Some(t) if t.contains("change_id") => commit.change_id().hex(),
                    Some(_) => {
                        // Complex template — can't handle in-process
                        return Err("complex template not supported".into());
                    }
                    None => {
                        // Default: change_id + description first line
                        let desc_line = commit.description().lines().next().unwrap_or("");
                        format!("{} {}", commit.change_id().hex(), desc_line)
                    }
                };
                output.push_str(line.trim_end());
                output.push('\n');
            }

            Ok(JjOutput {
                stdout: output,
                stderr: String::new(),
                success: true,
            })
        }

        /// jj op log -n 1 -T ...
        fn jjlib_op(&self, _args: &[&str]) -> std::result::Result<JjOutput, String> {
            let repo = self.get_repo()?;
            let op = repo.operation();
            let op_id = op.id().hex();
            let description = &op.metadata().description;
            let output = format!("{}\t{}\n", op_id, description);

            Ok(JjOutput {
                stdout: output,
                stderr: String::new(),
                success: true,
            })
        }

        /// jj bookmark list [--all] -T ...
        fn jjlib_bookmark_list(&self, _args: &[&str]) -> std::result::Result<JjOutput, String> {
            let repo = self.get_repo()?;
            let view = repo.view();

            let mut output = String::new();
            for (name, target) in view.local_bookmarks() {
                let commit_id_hex = target
                    .as_normal()
                    .map(|id| id.hex())
                    .unwrap_or_else(|| "(conflicted)".into());
                output.push_str(&format!("{}|{}\n", name.as_str(), commit_id_hex));
            }

            Ok(JjOutput {
                stdout: output,
                stderr: String::new(),
                success: true,
            })
        }

        /// jj status — fall back, too complex for initial impl
        fn jjlib_status(&self) -> std::result::Result<JjOutput, String> {
            Err("status requires working copy diff, falling back".into())
        }

        /// jj show — fall back, requires diff formatting
        fn jjlib_show(&self, _args: &[&str]) -> std::result::Result<JjOutput, String> {
            Err("show requires diff formatting, falling back".into())
        }

        /// jj evolog — fall back for now
        fn jjlib_evolog(&self, _args: &[&str]) -> std::result::Result<JjOutput, String> {
            Err("evolog not yet implemented, falling back".into())
        }

        /// Evaluate a revset string and return matching commits
        fn eval_revset(
            &self,
            repo: &Arc<ReadonlyRepo>,
            revset_str: &str,
        ) -> std::result::Result<Vec<jj_lib::commit::Commit>, String> {
            let aliases_map = RevsetAliasesMap::new();
            let extensions = RevsetExtensions::new();
            let context = RevsetParseContext {
                aliases_map: &aliases_map,
                local_variables: HashMap::new(),
                user_email: "",
                date_pattern_context: DatePatternContext::from(chrono::Local::now()),
                default_ignored_remote: None,
                use_glob_by_default: false,
                extensions: &extensions,
                workspace: None,
            };

            let mut diagnostics = RevsetDiagnostics::new();
            let user_expr = revset::parse(&mut diagnostics, revset_str, &context)
                .map_err(|e| format!("revset parse: {e}"))?;

            let no_extensions: &[Box<dyn revset::SymbolResolverExtension>] = &[];
            let symbol_resolver = SymbolResolver::new(repo.as_ref(), no_extensions);

            let resolved = user_expr
                .resolve_user_expression(repo.as_ref(), &symbol_resolver)
                .map_err(|e| format!("revset resolve: {e}"))?;

            let revset_result = resolved
                .evaluate(repo.as_ref())
                .map_err(|e| format!("revset evaluate: {e}"))?;

            let mut commits = Vec::new();
            for commit_or_err in revset_result.iter().commits(repo.store()) {
                match commit_or_err {
                    Ok(commit) => commits.push(commit),
                    Err(e) => return Err(format!("commit fetch: {e}")),
                }
            }

            Ok(commits)
        }

        /// Extract a flag value from args: e.g. extract_arg(&["-r", "@"], "-r") => Some("@")
        fn extract_arg<'a>(args: &[&'a str], flag: &str) -> Option<&'a str> {
            args.iter()
                .position(|a| *a == flag)
                .and_then(|i| args.get(i + 1).copied())
        }
    }

    #[async_trait]
    impl JjExecutor for JjLibExecutor {
        #[instrument(skip(self), fields(repo = %self.repo_root.display()))]
        async fn exec(&self, args: &[&str]) -> Result<JjOutput> {
            match Self::classify(args) {
                OpKind::Read => match self.try_jjlib_read(args).await {
                    Ok(out) => {
                        debug!("jj-lib read succeeded for {:?}", args);
                        Ok(out)
                    }
                    Err(e) => {
                        warn!("jj-lib read failed ({}), falling back to CLI", e);
                        self.fallback.exec(args).await
                    }
                },
                OpKind::Write => {
                    debug!("write operation {:?}, using CLI + invalidating cache", args);
                    self.invalidate_cache();
                    self.fallback.exec(args).await
                }
            }
        }

        fn repo_root(&self) -> &PathBuf {
            &self.repo_root
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_classify_reads() {
            assert_eq!(JjLibExecutor::classify(&["log", "-r", "@"]), OpKind::Read);
            assert_eq!(
                JjLibExecutor::classify(&["log", "-r", "@", "-T", "description"]),
                OpKind::Read
            );
            assert_eq!(
                JjLibExecutor::classify(&["op", "log", "-n", "1"]),
                OpKind::Read
            );
            assert_eq!(
                JjLibExecutor::classify(&["bookmark", "list", "--all"]),
                OpKind::Read
            );
            assert_eq!(JjLibExecutor::classify(&["status"]), OpKind::Read);
            assert_eq!(JjLibExecutor::classify(&["show"]), OpKind::Read);
            assert_eq!(JjLibExecutor::classify(&["evolog"]), OpKind::Read);
        }

        #[test]
        fn test_classify_writes() {
            assert_eq!(
                JjLibExecutor::classify(&["describe", "-m", "test"]),
                OpKind::Write
            );
            assert_eq!(JjLibExecutor::classify(&["new"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["commit"]), OpKind::Write);
            assert_eq!(
                JjLibExecutor::classify(&["bookmark", "create", "foo"]),
                OpKind::Write
            );
            assert_eq!(
                JjLibExecutor::classify(&["bookmark", "set", "foo"]),
                OpKind::Write
            );
            assert_eq!(
                JjLibExecutor::classify(&["bookmark", "delete", "foo"]),
                OpKind::Write
            );
            assert_eq!(JjLibExecutor::classify(&["squash"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["absorb"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["split"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["parallelize"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["undo"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["restore"]), OpKind::Write);
            assert_eq!(JjLibExecutor::classify(&["fix"]), OpKind::Write);
            assert_eq!(
                JjLibExecutor::classify(&["git", "push"]),
                OpKind::Write
            );
            assert_eq!(JjLibExecutor::classify(&[]), OpKind::Write);
        }

        #[test]
        fn test_classify_describe_always_write() {
            // Explicit test: describe is NOT in the read list
            assert_eq!(
                JjLibExecutor::classify(&["describe", "-r", "@"]),
                OpKind::Write
            );
        }

        #[test]
        fn test_classify_op_subcommands() {
            assert_eq!(
                JjLibExecutor::classify(&["op", "log"]),
                OpKind::Read
            );
            assert_eq!(
                JjLibExecutor::classify(&["op", "restore"]),
                OpKind::Write
            );
            assert_eq!(
                JjLibExecutor::classify(&["op", "undo"]),
                OpKind::Write
            );
        }

        #[test]
        fn test_extract_arg() {
            assert_eq!(
                JjLibExecutor::extract_arg(&["log", "-r", "@", "-T", "description"], "-r"),
                Some("@")
            );
            assert_eq!(
                JjLibExecutor::extract_arg(&["log", "-r", "@", "-T", "description"], "-T"),
                Some("description")
            );
            assert_eq!(
                JjLibExecutor::extract_arg(&["log", "-r", "@"], "-T"),
                None
            );
        }

        #[test]
        fn test_constructor() {
            let executor = JjLibExecutor::new("/tmp/test");
            assert_eq!(executor.repo_root(), &PathBuf::from("/tmp/test"));
        }

        #[test]
        fn test_cache_invalidation() {
            let executor = JjLibExecutor::new("/tmp/test");
            // Initially no cached handle
            assert!(executor.handle.lock().unwrap().is_none());
            // invalidate_cache is a no-op on empty cache (doesn't panic)
            executor.invalidate_cache();
            assert!(executor.handle.lock().unwrap().is_none());
        }
    }
}

#[cfg(feature = "jj-lib-integration")]
pub use inner::JjLibExecutor;
