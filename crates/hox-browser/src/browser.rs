//! Browser lifecycle management using Chrome DevTools Protocol

use crate::error::Result;
use hox_core::HoxError;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Configuration for browser launch
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// Run in headless mode (default: true)
    pub headless: bool,
    /// Browser window width
    pub window_width: u32,
    /// Browser window height
    pub window_height: u32,
    /// User agent string
    pub user_agent: Option<String>,
    /// Navigation timeout in seconds
    pub timeout_seconds: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            headless: true,
            window_width: 1920,
            window_height: 1080,
            user_agent: None,
            timeout_seconds: 30,
        }
    }
}

/// Active browser session with Chrome DevTools Protocol
pub struct BrowserSession {
    /// Underlying browser instance (kept alive for tab lifetime)
    #[allow(dead_code)]
    browser: Browser,
    /// Current active tab
    tab: Arc<Tab>,
    /// Configuration
    config: BrowserConfig,
}

impl BrowserSession {
    /// Launch a new browser instance
    ///
    /// # Example
    /// ```no_run
    /// use hox_browser::browser::BrowserSession;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let session = BrowserSession::launch().await.unwrap();
    ///     session.navigate("https://example.com").await.unwrap();
    /// }
    /// ```
    pub async fn launch() -> Result<Self> {
        Self::launch_with_config(BrowserConfig::default()).await
    }

    /// Launch browser with custom configuration
    pub async fn launch_with_config(config: BrowserConfig) -> Result<Self> {
        info!(
            "Launching browser (headless: {}, size: {}x{})",
            config.headless, config.window_width, config.window_height
        );

        let mut launch_options = LaunchOptions::default_builder()
            .headless(config.headless)
            .window_size(Some((config.window_width, config.window_height)))
            .build()
            .map_err(|e| HoxError::Browser(format!("Failed to launch browser: {}", e)))?;

        // Add user agent if specified
        let user_agent_arg: Option<String> = config.user_agent.as_ref().map(|ua| format!("--user-agent={}", ua));
        if let Some(ref ua_arg) = user_agent_arg {
            launch_options.args.push(OsStr::new(ua_arg));
        }

        // Launch browser
        let browser = Browser::new(launch_options)
            .map_err(|e| HoxError::Browser(format!("Failed to launch browser: {}", e)))?;

        // Get initial tab
        let tab = browser
            .new_tab()
            .map_err(|e| HoxError::Browser(format!("Failed to create tab: {}", e)))?;

        info!("Browser launched successfully");

        Ok(Self {
            browser,
            tab,
            config,
        })
    }

    /// Connect to an existing browser instance
    ///
    /// # Arguments
    /// * `port` - Chrome DevTools Protocol port (typically 9222)
    pub async fn connect(port: u16) -> Result<Self> {
        info!("Connecting to existing browser on port {}", port);

        let browser = Browser::connect(format!("http://127.0.0.1:{}", port))
            .map_err(|e| HoxError::Browser(format!("Failed to connect to browser: {}", e)))?;

        let tab = browser
            .new_tab()
            .map_err(|e| HoxError::Browser(format!("Failed to create tab: {}", e)))?;

        info!("Connected to browser successfully");

        Ok(Self {
            browser,
            tab,
            config: BrowserConfig::default(),
        })
    }

    /// Navigate to a URL
    ///
    /// # Arguments
    /// * `url` - URL to navigate to
    pub async fn navigate(&self, url: &str) -> Result<()> {
        debug!("Navigating to {}", url);

        self.tab
            .navigate_to(url)
            .map_err(|e| HoxError::Browser(format!("Failed to navigate to {}: {}", url, e)))?;

        // Wait for navigation to complete
        self.tab
            .wait_until_navigated()
            .map_err(|e| HoxError::Browser(format!("Navigation timeout for {}: {}", url, e)))?;

        info!("Successfully navigated to {}", url);
        Ok(())
    }

    /// Wait for an element to appear
    ///
    /// # Arguments
    /// * `selector` - CSS selector for the element
    /// * `timeout` - Optional timeout duration (uses config default if None)
    pub async fn wait_for_element(&self, selector: &str, timeout: Option<Duration>) -> Result<()> {
        let timeout_duration = timeout.unwrap_or_else(|| Duration::from_secs(self.config.timeout_seconds));

        debug!("Waiting for element: {} (timeout: {:?})", selector, timeout_duration);

        self.tab
            .wait_for_element_with_custom_timeout(selector, timeout_duration)
            .map_err(|_e| HoxError::Browser(format!("Element not found: {}", selector)))?;

        debug!("Element found: {}", selector);
        Ok(())
    }

    /// Execute JavaScript in the page context
    ///
    /// # Arguments
    /// * `script` - JavaScript code to execute
    ///
    /// # Returns
    /// JSON result from JavaScript execution
    pub async fn evaluate_script(&self, script: &str) -> Result<serde_json::Value> {
        debug!("Evaluating JavaScript: {}", script);

        let result = self
            .tab
            .evaluate(script, false)
            .map_err(|e| HoxError::Browser(format!("JavaScript evaluation failed: {}", e)))?;

        Ok(result.value.unwrap_or(serde_json::Value::Null))
    }

    /// Get the current page title
    pub async fn get_title(&self) -> Result<String> {
        let result = self.evaluate_script("document.title").await?;
        Ok(result.as_str().unwrap_or("").to_string())
    }

    /// Get the current URL
    pub async fn get_url(&self) -> Result<String> {
        let result = self.evaluate_script("window.location.href").await?;
        Ok(result.as_str().unwrap_or("").to_string())
    }

    /// Check if an element exists
    ///
    /// # Arguments
    /// * `selector` - CSS selector for the element
    pub async fn element_exists(&self, selector: &str) -> bool {
        self.tab.wait_for_element(selector).is_ok()
    }

    /// Get text content of an element
    ///
    /// # Arguments
    /// * `selector` - CSS selector for the element
    pub async fn get_text_content(&self, selector: &str) -> Result<String> {
        let script = format!("document.querySelector('{}')?.textContent", selector);
        let result = self.evaluate_script(&script).await?;
        Ok(result.as_str().unwrap_or("").to_string())
    }

    /// Get reference to the active tab
    pub fn tab(&self) -> &Arc<Tab> {
        &self.tab
    }

    /// Close the browser session
    pub async fn close(self) -> Result<()> {
        info!("Closing browser session");
        // Browser will be dropped and cleaned up automatically
        Ok(())
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        debug!("BrowserSession dropped, browser will be cleaned up");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BrowserConfig::default();
        assert!(config.headless);
        assert_eq!(config.window_width, 1920);
        assert_eq!(config.window_height, 1080);
        assert_eq!(config.timeout_seconds, 30);
    }

    #[test]
    fn test_custom_config() {
        let config = BrowserConfig {
            headless: false,
            window_width: 1024,
            window_height: 768,
            user_agent: Some("CustomAgent/1.0".to_string()),
            timeout_seconds: 60,
        };

        assert!(!config.headless);
        assert_eq!(config.window_width, 1024);
        assert!(config.user_agent.is_some());
    }
}
