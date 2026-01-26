# hox-browser

Browser automation and visual verification for Hox orchestration.

## Overview

`hox-browser` provides browser automation capabilities using Chrome DevTools Protocol (CDP) for UI verification tasks in the Hox orchestration system. It integrates with the artifact storage system from `hox-agent` to capture and store screenshots as validation artifacts.

## Features

- **Browser Management**: Launch and control Chrome/Chromium browsers
- **Screenshot Capture**: Full-page and element-specific screenshots via CDP
- **Visual Verification**: Element existence checks, text validation, attribute verification
- **Artifact Storage**: Seamless integration with `hox-agent` artifact system
- **Headless Operation**: Default headless mode with optional GUI support

## Architecture

```
hox-browser
├── browser.rs       - Browser lifecycle management (launch, connect, navigate)
├── screenshot.rs    - Screenshot capture with artifact storage
├── verification.rs  - Visual verification helpers (element checks, assertions)
└── error.rs         - Error types for browser operations
```

## Usage

### Basic Browser Session

```rust
use hox_browser::browser::BrowserSession;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Launch browser
    let session = BrowserSession::launch().await?;

    // Navigate to page
    session.navigate("https://example.com").await?;

    // Get page title
    let title = session.get_title().await?;
    println!("Page title: {}", title);

    // Close browser
    session.close().await?;

    Ok(())
}
```

### Capture Screenshots

```rust
use hox_browser::browser::BrowserSession;
use hox_browser::screenshot::{capture_full_page, capture_element};
use hox_agent::ArtifactManager;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = BrowserSession::launch().await?;
    session.navigate("https://example.com").await?;

    let manager = ArtifactManager::new(PathBuf::from(".hox"));

    // Full page screenshot
    let screenshot = capture_full_page(
        &session,
        &manager,
        "change-123",
        "homepage"
    ).await?;

    println!("Screenshot saved: {:?}", screenshot.path);

    // Element screenshot
    let button_screenshot = capture_element(
        &session,
        &manager,
        "change-123",
        "save-button",
        ".save-button"
    ).await?;

    session.close().await?;
    Ok(())
}
```

### Visual Verification

```rust
use hox_browser::browser::BrowserSession;
use hox_browser::verification::{verify_element, verify_text};
use hox_agent::ArtifactManager;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = BrowserSession::launch().await?;
    session.navigate("https://example.com").await?;

    let manager = ArtifactManager::new(PathBuf::from(".hox"));

    // Verify element exists and capture screenshot
    let check = verify_element(
        &session,
        ".main-heading",
        "change-123",
        Some(&manager)
    ).await?;

    if check.passed() {
        println!("Element found: {:?}", check.text_content);
        println!("Screenshot: {:?}", check.screenshot);
    }

    // Verify text content
    let text_matches = verify_text(
        &session,
        "h1",
        "Welcome"
    ).await?;

    assert!(text_matches);

    session.close().await?;
    Ok(())
}
```

### Custom Browser Configuration

```rust
use hox_browser::browser::{BrowserSession, BrowserConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BrowserConfig {
        headless: false,  // Show browser window
        window_width: 1024,
        window_height: 768,
        user_agent: Some("CustomBot/1.0".to_string()),
        timeout_seconds: 60,
    };

    let session = BrowserSession::launch_with_config(config).await?;
    // ... use session

    session.close().await?;
    Ok(())
}
```

## Integration with Artifact System

Screenshots are automatically stored in `.hox/artifacts/{change-id}/` using the `ArtifactManager` from `hox-agent`. Each screenshot gets:

- **Unique filename**: `{timestamp}-screenshot.png`
- **Metadata**: Size, MIME type, creation time, description
- **Storage path**: Relative to `.hox/artifacts/`

Example artifact storage:

```
.hox/artifacts/
└── change-abc123/
    ├── 20260125-143022-screenshot.png  (full page)
    └── 20260125-143025-screenshot.png  (element)
```

## Requirements

- Chrome or Chromium browser installed
- For headless operation: no additional setup required
- For connecting to existing browser: Launch Chrome with `--remote-debugging-port=9222`

## Error Handling

All browser operations return `Result<T, BrowserError>` with specific error types:

- `LaunchFailed` - Browser failed to start
- `ConnectionFailed` - Could not connect to existing browser
- `NavigationFailed` - Page navigation error
- `ScreenshotFailed` - Screenshot capture error
- `ElementNotFound` - CSS selector didn't match any element
- `Timeout` - Operation exceeded timeout
- `JavaScriptError` - JavaScript evaluation failed

## Testing

```bash
# Check compilation
cargo check -p hox-browser

# Run tests
cargo test -p hox-browser

# Run tests with output
cargo test -p hox-browser -- --nocapture
```

## Optional Dependency

`hox-browser` is an optional capability in the Hox workspace. Agents that don't need browser automation can skip this dependency.

To use in your crate:

```toml
[dependencies]
hox-browser = { workspace = true }
hox-agent = { workspace = true }  # Required for ArtifactManager
```

## CDP Protocol

This crate uses the Chrome DevTools Protocol (CDP) via the `headless_chrome` library for all browser automation. CDP provides:

- Full browser control
- Screenshot capture
- JavaScript execution
- Element inspection
- Network monitoring (future capability)

## Future Enhancements

Potential additions (not yet implemented):

- Accessibility tree capture
- Performance metrics logging
- Network request interception
- Console log capture
- Cookie/localStorage management
- Mobile device emulation
- Screenshot comparison (visual regression testing)
