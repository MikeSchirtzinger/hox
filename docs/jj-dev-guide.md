# Complete Guide to JJ Parallel Development Pattern (jj-dev) 

A comprehensive walkthrough of building software in parallel using JJ conventions.

---

## 📖 Table of Contents

1. [Prerequisites](#prerequisites)
2. [Understanding the Pattern](#understanding-the-pattern)
3. [Walkthrough: Building a Blog Platform](#walkthrough-building-a-blog-platform)
4. [Common Scenarios](#common-scenarios)
5. [Best Practices](#best-practices)
6. [Troubleshooting](#troubleshooting)
7. [Advanced Patterns](#advanced-patterns)

---

## Prerequisites

### Install JJ

**macOS**:
```bash
brew install jj
```

**Linux** (Arch):
```bash
pacman -S jj
```

**Linux** (other) / **Windows**:
```bash
cargo install --git https://github.com/martinvonz/jj jj-cli
```

### Verify Installation
```bash
jj --version
# Should show: jj 0.x.x or higher
```

### Initialize a Repository

**New project**:
```bash
mkdir my-project && cd my-project
jj git init
jj describe -m "Initial commit"
```

**Existing Git project**:
```bash
cd my-existing-project
jj git init --git-repo=.
```

---

## Understanding the Pattern

### The Core Idea

Instead of working sequentially:
```
Step 1 → Step 2 → Step 3 → Step 4 = Long time
```

Work in parallel using JJ bookmarks:
```
Step 1 ┐
Step 2 ├─→ Merge = Much faster!
Step 3 ┘
Step 4 ┘
```

### The Five Conventions

1. **Bookmark naming**: `{actor}-{component}`
2. **Commit messages**: `{Actor}: {description}`
3. **Change descriptions**: `{Component} - {brief}`
4. **Work protocol**: Create, commit, keep bookmark
5. **Merge protocol**: Combine bookmarks when ready

These conventions make it easy to:
- See who's working on what
- Track progress without tools
- Merge cleanly
- Maintain clear history

---

## JJ-Native Workflow vs Git Compatibility

### The Core Philosophy

JJ treats the working copy as a **continuous commit**—your edits are always part of a change. This differs fundamentally from Git's staged/unstaged model.

**The jj-native pattern:**
```bash
# 1. Describe your intent FIRST
jj describe -m "Implement user authentication"

# 2. Work until the goal is complete
# ... edit files, test, iterate ...

# 3. When done, start a NEW change (finalizes the current one)
jj new -m "Next task description"
```

**Why this is better than `jj commit`:**

| Aspect | `jj describe` + `jj new` | `jj commit` |
|--------|--------------------------|-------------|
| Mutability | Changes stay editable until you move on | Feels "final" (but isn't) |
| Intent clarity | Describe goal upfront, work toward it | Message comes after work |
| History surgery | Easy to split, squash, rebase later | Same, but mental model differs |
| Undo safety | `jj new` is always additive | `jj commit` amends when dirty |

### When to Use `jj commit` (Git Colocated Mode)

In **colocated repositories** (JJ + Git sharing the same `.git`), `jj commit` serves a specific purpose: it updates Git's `HEAD` reference.

**Use `jj commit` when:**
- You need Git tooling to see your latest work (CI, hooks, IDE Git integration)
- You're pushing to a Git remote and want `HEAD` aligned
- You're collaborating with Git-only users

**Example colocated workflow:**
```bash
# Initialize colocated repo
jj git init --git-repo=.

# Work normally with jj-native commands
jj describe -m "Add login endpoint"
# ... work ...

# When ready to push/share via Git, use jj commit
jj commit -m "Add login endpoint"  # Updates Git HEAD
jj git push
```

**For pure JJ repos or when Git HEAD doesn't matter, prefer `jj new`.**

---

## Walkthrough: Building a Blog Platform

Let's build a complete blog platform with:
- Database (PostgreSQL + schema)
- API (FastAPI endpoints)
- UI (React components)
- Tests (pytest + jest)

### Phase 1: Setup (Coordinator/Lead)

**Identify components**:
```
1. Database: PostgreSQL schema for posts, users, comments
2. API: FastAPI CRUD endpoints
3. UI: React components for reading/writing posts
4. Tests: Full test coverage
```

**Plan parallel work**:
```
Agent 1: Database (independent)
Agent 2: API (depends on database schema, but can start with interface)
Agent 3: UI (depends on API interface)
Agent 4: Tests (can work in parallel once interfaces are defined)
```

**Create bookmarks**:
```bash
jj bookmark create agent-1-database
jj bookmark create agent-2-api
jj bookmark create agent-3-ui
jj bookmark create agent-4-tests
```

### Phase 2: Parallel Execution

#### Agent 1: Database

```bash
# Create bookmark and describe intent
jj new main
jj bookmark set agent-1-database
jj describe -m "Database - PostgreSQL schema for blog"

# Create database schema
cat > schema.sql <<'EOF'
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    title VARCHAR(200) NOT NULL,
    content TEXT NOT NULL,
    author_id INTEGER REFERENCES users(id),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE comments (
    id SERIAL PRIMARY KEY,
    content TEXT NOT NULL,
    post_id INTEGER REFERENCES posts(id),
    author_id INTEGER REFERENCES users(id),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
EOF

# Finalize schema work, start models work
jj new
jj describe -m "Agent 1: Add SQLAlchemy models"

# Create SQLAlchemy models
cat > models.py <<'EOF'
from sqlalchemy import Column, Integer, String, Text, ForeignKey, DateTime
from sqlalchemy.ext.declarative import declarative_base
from datetime import datetime

Base = declarative_base()

class User(Base):
    __tablename__ = 'users'
    id = Column(Integer, primary_key=True)
    username = Column(String(50), unique=True, nullable=False)
    email = Column(String(100), unique=True, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow)

class Post(Base):
    __tablename__ = 'posts'
    id = Column(Integer, primary_key=True)
    title = Column(String(200), nullable=False)
    content = Column(Text, nullable=False)
    author_id = Column(Integer, ForeignKey('users.id'))
    created_at = Column(DateTime, default=datetime.utcnow)

class Comment(Base):
    __tablename__ = 'comments'
    id = Column(Integer, primary_key=True)
    content = Column(Text, nullable=False)
    post_id = Column(Integer, ForeignKey('posts.id'))
    author_id = Column(Integer, ForeignKey('users.id'))
    created_at = Column(DateTime, default=datetime.utcnow)
EOF

# Finalize and update bookmark
jj new
jj bookmark set agent-1-database -r @-  # Point bookmark to completed work
```

#### Agent 2: API (Simultaneously!)

```bash
# Create bookmark and describe intent
jj new main
jj bookmark set agent-2-api
jj describe -m "API - FastAPI REST endpoints"

# Create API structure
cat > main.py <<'EOF'
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import List

app = FastAPI()

class UserCreate(BaseModel):
    username: str
    email: str

class PostCreate(BaseModel):
    title: str
    content: str
    author_id: int

class CommentCreate(BaseModel):
    content: str
    post_id: int
    author_id: int

@app.post("/users")
async def create_user(user: UserCreate):
    # TODO: Integrate with database
    return {"id": 1, **user.dict()}

@app.get("/users/{user_id}")
async def get_user(user_id: int):
    # TODO: Integrate with database
    return {"id": user_id, "username": "example"}

@app.post("/posts")
async def create_post(post: PostCreate):
    # TODO: Integrate with database
    return {"id": 1, **post.dict()}

@app.get("/posts")
async def list_posts():
    # TODO: Integrate with database
    return []

@app.post("/comments")
async def create_comment(comment: CommentCreate):
    # TODO: Integrate with database
    return {"id": 1, **comment.dict()}
EOF

# Finalize endpoints, start database integration
jj new
jj describe -m "Agent 2: Add database connection"

# Add database integration (after agent 1 completes schema)
cat > database.py <<'EOF'
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker

DATABASE_URL = "postgresql://user:pass@localhost/blog"
engine = create_engine(DATABASE_URL)
SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)

def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()
EOF

# Finalize and update bookmark
jj new
jj bookmark set agent-2-api -r @-
```

#### Agent 3: UI (Simultaneously!)

```bash
# Create bookmark and describe intent
jj new main
jj bookmark set agent-3-ui
jj describe -m "UI - React components for blog"

# Create React components
cat > PostList.jsx <<'EOF'
import React, { useEffect, useState } from 'react';

export function PostList() {
    const [posts, setPosts] = useState([]);

    useEffect(() => {
        fetch('/api/posts')
            .then(res => res.json())
            .then(data => setPosts(data));
    }, []);

    return (
        <div className="post-list">
            <h1>Blog Posts</h1>
            {posts.map(post => (
                <article key={post.id}>
                    <h2>{post.title}</h2>
                    <p>{post.content}</p>
                </article>
            ))}
        </div>
    );
}
EOF

# Finalize PostList, start PostEditor
jj new
jj describe -m "Agent 3: Add PostEditor component"

cat > PostEditor.jsx <<'EOF'
import React, { useState } from 'react';

export function PostEditor() {
    const [title, setTitle] = useState('');
    const [content, setContent] = useState('');

    const handleSubmit = async (e) => {
        e.preventDefault();
        await fetch('/api/posts', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ title, content, author_id: 1 })
        });
        setTitle('');
        setContent('');
    };

    return (
        <form onSubmit={handleSubmit}>
            <input
                value={title}
                onChange={e => setTitle(e.target.value)}
                placeholder="Title"
            />
            <textarea
                value={content}
                onChange={e => setContent(e.target.value)}
                placeholder="Content"
            />
            <button type="submit">Publish</button>
        </form>
    );
}
EOF

# Finalize and update bookmark
jj new
jj bookmark set agent-3-ui -r @-
```

#### Agent 4: Tests (Simultaneously!)

```bash
# Create bookmark and describe intent
jj new main
jj bookmark set agent-4-tests
jj describe -m "Tests - pytest and jest test suites"

# Create API tests
cat > test_api.py <<'EOF'
import pytest
from fastapi.testclient import TestClient
from main import app

client = TestClient(app)

def test_create_user():
    response = client.post("/users", json={
        "username": "testuser",
        "email": "test@example.com"
    })
    assert response.status_code == 200
    assert response.json()["username"] == "testuser"

def test_create_post():
    response = client.post("/posts", json={
        "title": "Test Post",
        "content": "This is a test",
        "author_id": 1
    })
    assert response.status_code == 200
    assert response.json()["title"] == "Test Post"

def test_list_posts():
    response = client.get("/posts")
    assert response.status_code == 200
    assert isinstance(response.json(), list)
EOF

# Finalize API tests, start UI tests
jj new
jj describe -m "Agent 4: Add UI tests"

# Create UI tests
cat > PostList.test.jsx <<'EOF'
import { render, screen } from '@testing-library/react';
import { PostList } from './PostList';

test('renders post list', async () => {
    render(<PostList />);
    expect(screen.getByText('Blog Posts')).toBeInTheDocument();
});
EOF

# Finalize and update bookmark
jj new
jj bookmark set agent-4-tests -r @-
```

### Phase 3: Check Progress

```bash
# See all bookmarks
jj bookmark list

# See what each agent did
jj log -r 'bookmarks()' --limit 20

# Compare specific bookmark to main
jj diff -r main -r agent-1-database

# Visual graph
jj log --graph --limit 15
```

### Phase 4: Merging

#### Option 1: Merge All at Once (Parallel Components)

```bash
# Create a merge of all four components
jj new agent-1-database agent-2-api agent-3-ui agent-4-tests
jj describe -m "Integrate complete blog platform"

# If there are conflicts, resolve them
jj status  # Shows conflicts if any
# ... edit conflicted files ...
# (conflicts resolve automatically when you save the file)

# Test the integrated system
pytest
npm test

# If tests pass, finalize and update main
jj new
jj bookmark set main -r @-
```

#### Option 2: Sequential Integration (Dependent Components)

```bash
# First: Database (foundation)
jj new main agent-1-database -m "Integrate database layer"
pytest tests/test_database.py

# Second: API (needs database)
jj new @ agent-2-api -m "Integrate API layer"
pytest tests/test_api.py

# Third: UI (needs API)
jj new @ agent-3-ui -m "Integrate UI layer"
npm test

# Fourth: Tests (validate everything)
jj new @ agent-4-tests -m "Add comprehensive tests"
pytest && npm test

# Update main bookmark
jj bookmark set main
```

### Phase 5: Cleanup

```bash
# Remove agent bookmarks (optional)
jj bookmark delete agent-1-database
jj bookmark delete agent-2-api
jj bookmark delete agent-3-ui
jj bookmark delete agent-4-tests

# Or keep them for reference
```

---

## Common Scenarios

### Scenario 1: Adding a Feature to Existing Project

```bash
# Create bookmark and describe intent
jj new main
jj bookmark set alice-user-profile
jj describe -m "User Profile - Avatar upload and bio editing"

# Make changes
# ... edit files for avatar upload ...

# Finalize avatar work, start profile editing
jj new
jj describe -m "Alice: Add profile editing UI"
# ... edit files for profile UI ...

# Finalize and update bookmark
jj new
jj bookmark set alice-user-profile -r @-

# When ready, merge to main
jj new main alice-user-profile -m "Add user profile feature"
```

### Scenario 2: Fixing Multiple Bugs in Parallel

```bash
# Bug 1: Login timeout
jj new main
jj bookmark set bug-123-login-timeout
jj describe -m "Fix: Increase login timeout to 30s"
# ... fix ...
jj new
jj bookmark set bug-123-login-timeout -r @-

# Bug 2: Comment sorting (parallel to bug 1)
jj new main
jj bookmark set bug-124-comment-sort
jj describe -m "Fix: Sort comments by timestamp DESC"
# ... fix ...
jj new
jj bookmark set bug-124-comment-sort -r @-

# Merge fixes
jj new main bug-123-login-timeout bug-124-comment-sort \
  -m "Fix login and comment bugs"
```

### Scenario 3: Refactoring While Adding Features

```bash
# Refactor bookmark
jj new main
jj bookmark set refactor-models
jj describe -m "Refactor: Extract base model class"
# ... refactor ...
jj new
jj bookmark set refactor-models -r @-

# Feature bookmark (parallel)
jj new main
jj bookmark set feature-notifications
jj describe -m "Feature: Add email notifications"
# ... build feature ...
jj new
jj bookmark set feature-notifications -r @-

# Merge (may have conflicts)
jj new main refactor-models feature-notifications \
  -m "Integrate refactoring and notifications"
```

### Scenario 4: AI Building Multiple Agents

```bash
# Agent 1 executes
jj new main
jj bookmark set agent-1-backend
jj describe -m "Backend - FastAPI application"
# ... builds backend ...
jj new
jj bookmark set agent-1-backend -r @-

# Agent 2 executes (parallel)
jj new main
jj bookmark set agent-2-frontend
jj describe -m "Frontend - React SPA"
# ... builds frontend ...
jj new
jj bookmark set agent-2-frontend -r @-

# Agent 3 executes (parallel)
jj new main
jj bookmark set agent-3-deployment
jj describe -m "Deployment - Docker and CI/CD"
# ... creates deployment ...
jj new
jj bookmark set agent-3-deployment -r @-

# AI merges all completed work
jj new agent-1-backend agent-2-frontend agent-3-deployment \
  -m "Complete application with deployment"
```

---

## Best Practices

### 1. Keep Bookmarks Independent
✅ **Good**: Database, API, UI as separate bookmarks
❌ **Bad**: Everything in one bookmark

### 2. Use Descriptive Names
✅ **Good**: `alice-payment-processing`
❌ **Bad**: `alice-stuff`

### 3. Create Small, Focused Changes
```bash
# Multiple small changes (jj-native)
jj describe -m "Agent 1: Add user model"
# ... work ...
jj new
jj describe -m "Agent 1: Add user repository"
# ... work ...
jj new
jj describe -m "Agent 1: Add user tests"

# NOT one giant change
# ❌ jj describe -m "Agent 1: Everything"
```

### 4. Test Before Merging
```bash
# Always test before merge
jj new agent-1-models agent-2-routes
pytest  # Make sure tests pass
jj describe -m "Integrate models and routes"
jj new  # Finalize the merge
```

### 5. Use Meaningful Merge Messages
✅ **Good**: `"Integrate auth system with user management"`
❌ **Bad**: `"Merge stuff"`

### 6. Keep Main Clean
```bash
# DON'T work directly on main without a bookmark
jj new main  # then immediately edit → ❌ Bad

# DO create bookmarks first
jj new main
jj bookmark set my-feature
jj describe -m "My Feature"  # ✅ Good
```

---

## Troubleshooting

### Problem: Merge Conflicts

**Symptoms**: After `jj new bookmark-1 bookmark-2`, you see conflicts

**Solution**:
```bash
# See conflicted files
jj status

# JJ shows conflicts inline in files
# Edit files to resolve conflicts
vim conflicted-file.py

# Conflicts are automatically resolved when you save
jj status  # Should show no conflicts

# The change already has your resolution - move on
jj new  # Start next work
```

### Problem: Lost Bookmark

**Symptoms**: Can't find a bookmark you created

**Solution**:
```bash
# List all bookmarks
jj bookmark list

# Search log for your work
jj log -r 'author("your-name")'

# Recreate bookmark if needed
jj bookmark create my-bookmark -r <change-id>
```

### Problem: Wrong Parent for Bookmark

**Symptoms**: Created bookmark from wrong starting point

**Solution**:
```bash
# Move bookmark to correct parent
jj rebase -r agent-1-database -d main
```

### Problem: Want to Undo Something

**Symptoms**: Made a mistake

**Solution**:
```bash
# See operation history
jj op log

# Undo last operation
jj op undo

# Undo specific operation
jj op undo --at <operation-id>
```

### Problem: Bookmarks Out of Sync

**Symptoms**: Bookmarks have diverged unexpectedly

**Solution**:
```bash
# See bookmark relationships
jj log --graph --limit 20

# Rebase bookmark onto main
jj rebase -r agent-1-database -d main
```

---

## Advanced Patterns

### Pattern 1: Dependent Bookmarks (Stacking)

When one bookmark depends on another:

```bash
# Base feature
jj new main
jj bookmark set base-auth
jj describe -m "Auth - Basic authentication"
# ... build auth ...
jj new
jj bookmark set base-auth -r @-

# Extended feature (depends on base-auth)
jj new base-auth
jj bookmark set extended-oauth
jj describe -m "Auth - OAuth integration"
# ... add OAuth ...
jj new
jj bookmark set extended-oauth -r @-

# Merge both (extended-oauth includes base-auth history)
jj new main extended-oauth -m "Add complete auth system"
```

### Pattern 2: Feature Flags with Bookmarks

```bash
# Main development
jj bookmark create feature-v1

# Experimental variation
jj bookmark create feature-v2-experiment
jj new feature-v1 -m "Experimental variation"
# ... try alternative approach ...

# Compare the two
jj diff -r feature-v1 -r feature-v2-experiment

# Choose one to merge
jj new main feature-v2-experiment -m "Use experimental approach"
```

### Pattern 3: Continuous Integration

```bash
# In CI/CD pipeline
#!/bin/bash

# Get all unmerged bookmarks
bookmarks=$(jj bookmark list | grep agent- | cut -d' ' -f1)

for bookmark in $bookmarks; do
    echo "Testing $bookmark..."

    # Create test merge
    jj new main $bookmark -m "Test merge $bookmark"

    # Run tests
    if pytest; then
        echo "✅ $bookmark passes tests"
    else
        echo "❌ $bookmark fails tests"
        exit 1
    fi

    # Clean up test merge
    jj abandon @
done

# If all pass, merge all
jj new main $bookmarks -m "Integrate all tested components"
```

### Pattern 4: Multi-Stage Development

```bash
# Stage 1: Prototypes (all parallel)
jj bookmark create proto-approach-a
jj bookmark create proto-approach-b
jj bookmark create proto-approach-c

# ... build prototypes ...

# Evaluate and choose
jj diff -r proto-approach-a -r proto-approach-b

# Stage 2: Develop chosen approach
jj bookmark create dev-chosen
jj new proto-approach-b -m "Develop chosen approach"

# Stage 3: Production-ready
jj new main dev-chosen -m "Production implementation"
```

---

## Next Steps

- **Try it**: Build something with this pattern
- **Share**: Show your team or community
- **Adapt**: Customize conventions for your workflow
- **Contribute**: Share your examples and improvements

---

## Additional Resources

- [JJ Documentation](https://martinvonz.github.io/jj/)
- [EXAMPLES.md](./EXAMPLES.md) - More real-world scenarios
- [REFERENCE.md](./REFERENCE.md) - Quick command reference
- [FAQ.md](./FAQ.md) - Common questions

---

**Happy parallel developing!** 🚀