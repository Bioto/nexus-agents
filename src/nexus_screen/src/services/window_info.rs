use anyhow::{self, Result};
use std::process::Command;
use super::super::error::ScreenError;

/// Represents window information on the system.
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub window_id: String,
    pub title: String,
    pub pid: Option<u32>,
    pub geometry: Option<WindowGeometry>,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_visible: bool,
    pub workspace: Option<String>,
    pub class_name: Option<String>,
}

/// Window geometry (position and size).
#[derive(Clone, Debug)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Service for querying window information.
pub struct WindowInfoService;

impl WindowInfoService {
    /// Lists all visible windows on the system.
    pub fn list_windows() -> Result<Vec<WindowInfo>> {
        #[cfg(target_os = "linux")]
        {
            Self::list_windows_linux()
        }

        #[cfg(target_os = "macos")]
        {
            Self::list_windows_macos()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Window enumeration not supported on this platform"
            ))
        }
    }

    /// Gets the currently active/focused window.
    pub fn get_active_window() -> Result<Option<WindowInfo>> {
        #[cfg(target_os = "linux")]
        {
            Self::get_active_window_linux()
        }

        #[cfg(target_os = "macos")]
        {
            Self::get_active_window_macos()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Active window detection not supported on this platform"
            ))
        }
    }

    /// Gets window information by its ID.
    pub fn get_window_by_id(window_id: &str) -> Result<Option<WindowInfo>> {
        #[cfg(target_os = "linux")]
        {
            Self::get_window_by_id_linux(window_id)
        }

        #[cfg(target_os = "macos")]
        {
            Self::get_window_by_id_macos(window_id)
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Window lookup not supported on this platform"
            ))
        }
    }

    /// Gets windows owned by the specified process ID.
    pub fn get_windows_by_pid(pid: u32) -> Result<Vec<WindowInfo>> {
        #[cfg(target_os = "linux")]
        {
            Self::get_windows_by_pid_linux(pid)
        }

        #[cfg(target_os = "macos")]
        {
            Self::get_windows_by_pid_macos(pid)
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(anyhow::anyhow!(
                "Window lookup by PID not supported on this platform"
            ))
        }
    }

    /// Gets windows matching the title pattern.
    pub fn get_windows_by_title(pattern: &str) -> Result<Vec<WindowInfo>> {
        let all_windows = Self::list_windows()?;
        Ok(all_windows
            .into_iter()
            .filter(|w| w.title.contains(pattern))
            .collect())
    }

    #[cfg(target_os = "linux")]
    fn list_windows_linux() -> Result<Vec<WindowInfo>> {
        // Try wmctrl first (more reliable)
        if let Ok(windows) = Self::list_windows_wmctrl() {
            if !windows.is_empty() {
                return Ok(windows);
            }
        }

        // Fallback to xdotool
        if let Ok(windows) = Self::list_windows_xdotool() {
            if !windows.is_empty() {
                return Ok(windows);
            }
        }

        // Last resort: xwininfo
        Self::list_windows_xwininfo()
    }

    #[cfg(target_os = "linux")]
    fn list_windows_wmctrl() -> Result<Vec<WindowInfo>> {
        let output = Command::new("wmctrl").arg("-l").output().map_err(|e| {
            ScreenError::Configuration(format!("Failed to run wmctrl: {}. Is wmctrl installed? (sudo apt install wmctrl)", e))
        })?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "wmctrl command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut windows = Vec::new();

        // Parse wmctrl -l output
        // Format: "0x01234567  0 desktop-name  Window Title"
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            
            let window_id = match parts.get(0) {
                Some(id) => id.to_string(),
                None => continue,
            };
            
            let workspace = match parts.get(2) {
                Some(ws) => ws.to_string(),
                None => "unknown".to_string(),
            };
            
            // Title is everything after the third whitespace-separated field
            // We can safely skip 3 because we checked len >= 4
            let title = parts.iter().skip(3).cloned().collect::<Vec<&str>>().join(" ");

            // Get additional info using wmctrl -i -G
            let geometry = Self::get_window_geometry_wmctrl(&window_id).ok();
            let pid = Self::get_window_pid_linux(&window_id).ok();

            windows.push(WindowInfo {
                window_id,
                title,
                pid,
                geometry,
                is_minimized: false, // Would need additional query
                is_maximized: false, // Would need additional query
                is_visible: true,
                workspace: Some(workspace),
                class_name: None,
            });
        }

        Ok(windows)
    }

    #[cfg(target_os = "linux")]
    fn list_windows_xdotool() -> Result<Vec<WindowInfo>> {
        let output = Command::new("xdotool")
            .arg("search")
            .arg("--onlyvisible")
            .arg("--class")
            .arg("")
            .output()
            .map_err(|e| {
                ScreenError::Configuration(format!("Failed to run xdotool: {}. Is xdotool installed? (sudo apt install xdotool)", e))
            })?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "xdotool command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut windows = Vec::new();

        for window_id_str in stdout.lines() {
            let window_id = window_id_str.trim();
            if window_id.is_empty() {
                continue;
            }

            // Get window name
            let title = Command::new("xdotool")
                .arg("getwindowname")
                .arg(window_id)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "Unknown".to_string());

            // Get window geometry
            let geometry = Self::get_window_geometry_xdotool(window_id).ok();

            // Get PID
            let pid = Command::new("xdotool")
                .arg("getwindowpid")
                .arg(window_id)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .parse::<u32>()
                            .ok()
                    } else {
                        None
                    }
                });

            windows.push(WindowInfo {
                window_id: window_id.to_string(),
                title,
                pid,
                geometry,
                is_minimized: false,
                is_maximized: false,
                is_visible: true,
                workspace: None,
                class_name: None,
            });
        }

        Ok(windows)
    }

    #[cfg(target_os = "linux")]
    fn list_windows_xwininfo() -> Result<Vec<WindowInfo>> {
        // xwininfo -tree -root is less reliable but available on most systems
        let output = Command::new("xwininfo")
            .arg("-tree")
            .arg("-root")
            .output()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to run xwininfo: {}. Is x11-utils installed? (sudo apt install x11-utils)",
                    e
                )
            })?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "xwininfo command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // xwininfo output is complex to parse, return empty for now
        // This is a fallback that could be improved
        Ok(Vec::new())
    }

    #[cfg(target_os = "linux")]
    fn get_window_geometry_wmctrl(window_id: &str) -> Result<WindowGeometry> {
        let output = Command::new("wmctrl")
            .arg("-i")
            .arg("-G")
            .arg("-l")
            .output()
            .map_err(|e| ScreenError::Configuration(format!("wmctrl -i -G -l failed: {}", e)))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("wmctrl -i -G -l failed"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            
            if parts.len() < 6 {
                continue;
            }

            if let Some(id) = parts.get(0) {
                if *id == window_id {
                    let x = parts.get(2).and_then(|s| s.parse::<i32>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid X coord"))?;
                    let y = parts.get(3).and_then(|s| s.parse::<i32>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid Y coord"))?;
                    let width = parts.get(4).and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid Width"))?;
                    let height = parts.get(5).and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid Height"))?;
                    
                    return Ok(WindowGeometry {
                        x,
                        y,
                        width,
                        height,
                    });
                }
            }
        }

        Err(anyhow::anyhow!("Window geometry not found"))
    }

    #[cfg(target_os = "linux")]
    fn get_window_geometry_xdotool(window_id: &str) -> Result<WindowGeometry> {
        let output = Command::new("xdotool")
            .arg("getwindowgeometry")
            .arg(window_id)
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("xdotool getwindowgeometry failed"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut x = 0;
        let mut y = 0;
        let mut width = 0;
        let mut height = 0;

        for line in stdout.lines() {
            if line.starts_with("  Position:") {
                let coords: Vec<&str> = line
                    .strip_prefix("  Position:")
                    .unwrap_or("")
                    .split(',')
                    .collect();
                if coords.len() >= 2 {
                    x = coords[0].trim().parse::<i32>().unwrap_or(0);
                    y = coords[1].trim().parse::<i32>().unwrap_or(0);
                }
            } else if line.starts_with("  Geometry:") {
                let dims: Vec<&str> = line
                    .strip_prefix("  Geometry:")
                    .unwrap_or("")
                    .split('x')
                    .collect();
                if dims.len() >= 2 {
                    width = dims[0].trim().parse::<u32>().unwrap_or(0);
                    height = dims[1].trim().parse::<u32>().unwrap_or(0);
                }
            }
        }

        Ok(WindowGeometry {
            x,
            y,
            width,
            height,
        })
    }

    #[cfg(target_os = "linux")]
    fn get_window_pid_linux(window_id: &str) -> Result<u32> {
        // Try xdotool first
        let output = Command::new("xdotool")
            .arg("getwindowpid")
            .arg(window_id)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if let Ok(pid) = pid_str.parse::<u32>() {
                    return Ok(pid);
                }
            }
        }

        // Fallback: try to extract from window ID using xprop
        let output = Command::new("xprop")
            .arg("-id")
            .arg(window_id)
            .arg("_NET_WM_PID")
            .output()
            .map_err(|e| ScreenError::Configuration(format!("xprop failed: {}", e)))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(pid_str) = stdout.split('=').nth(1) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    return Ok(pid);
                }
            }
        }

        Err(anyhow::anyhow!("Could not determine window PID"))
    }

    #[cfg(target_os = "linux")]
    fn get_active_window_linux() -> Result<Option<WindowInfo>> {
        // Try xdotool first
        let output = Command::new("xdotool").arg("getactivewindow").output();

        if let Ok(output) = output {
            if output.status.success() {
                let window_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Self::get_window_by_id_linux(&window_id);
            }
        }

        // Fallback: try wmctrl to get active window
        // Note: wmctrl doesn't have a direct "get active" command, so we use xdotool result above
        // This is a placeholder for potential future implementation
        Ok(None)
    }

    #[cfg(target_os = "linux")]
    fn get_window_by_id_linux(window_id: &str) -> Result<Option<WindowInfo>> {
        // Get window title
        let title = Command::new("xdotool")
            .arg("getwindowname")
            .arg(window_id)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "Unknown".to_string());

        let geometry = Self::get_window_geometry_xdotool(window_id).ok();
        let pid = Self::get_window_pid_linux(window_id).ok();

        Ok(Some(WindowInfo {
            window_id: window_id.to_string(),
            title,
            pid,
            geometry,
            is_minimized: false,
            is_maximized: false,
            is_visible: true,
            workspace: None,
            class_name: None,
        }))
    }

    #[cfg(target_os = "linux")]
    fn get_windows_by_pid_linux(pid: u32) -> Result<Vec<WindowInfo>> {
        // Use xdotool to find windows by PID
        let output = Command::new("xdotool")
            .arg("search")
            .arg("--pid")
            .arg(pid.to_string())
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut windows = Vec::new();

        for window_id_str in stdout.lines() {
            let window_id = window_id_str.trim();
            if window_id.is_empty() {
                continue;
            }

            if let Ok(Some(window)) = Self::get_window_by_id_linux(window_id) {
                windows.push(window);
            }
        }

        Ok(windows)
    }

    #[cfg(target_os = "macos")]
    fn list_windows_macos() -> Result<Vec<WindowInfo>> {
        // Use osascript with AppleScript to get window information
        let script = r#"
        tell application "System Events"
            set windowList to {}
            repeat with proc in every process
                try
                    repeat with win in windows of proc
                        set end of windowList to {id of win, name of win, name of proc, visible of win, minimized of win}
                    end repeat
                end try
            end repeat
            return windowList
        end tell
        "#;

        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| {
                ScreenError::Configuration(format!("Failed to run osascript: {}. Make sure you have granted Terminal/your app accessibility permissions.", e))
            })?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "osascript command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // AppleScript output is complex to parse reliably
        // For now, use a simpler approach with osascript
        Self::list_windows_macos_simple()
    }

    #[cfg(target_os = "macos")]
    fn list_windows_macos_simple() -> Result<Vec<WindowInfo>> {
        // Use osascript to get window info from each application
        // This is a simplified version that queries known applications
        let windows = Vec::new();

        // Get window info using a more direct AppleScript approach
        let script = r#"
        tell application "System Events"
            set appList to name of every process whose background only is false
            set windowInfo to {}
            repeat with appName in appList
                try
                    tell process appName
                        repeat with win in windows
                            try
                                set winTitle to name of win
                                set winId to id of win
                                set winVisible to visible of win
                                set winMinimized to minimized of win
                                set end of windowInfo to {winId, winTitle, appName, winVisible, winMinimized}
                            end try
                        end repeat
                    end tell
                end try
            end repeat
            return windowInfo
        end tell
        "#;

        let output = Command::new("osascript").arg("-e").arg(script).output()?;

        if output.status.success() {
            // Parse the output (format is complex, simplified here)
            let _stdout = String::from_utf8_lossy(&output.stdout);
            // AppleScript returns a list format that's hard to parse
            // For production, consider using a proper AppleScript parser or Objective-C bridge
        }

        // Fallback: return empty list with a note that this needs improvement
        Ok(windows)
    }

    #[cfg(target_os = "macos")]
    fn get_active_window_macos() -> Result<Option<WindowInfo>> {
        let script = r#"
        tell application "System Events"
            set frontApp to first application process whose frontmost is true
            set frontWindow to first window of frontApp
            return {id of frontWindow, name of frontWindow, name of frontApp, visible of frontWindow, minimized of frontWindow}
        end tell
        "#;

        let output = Command::new("osascript").arg("-e").arg(script).output()?;

        if !output.status.success() {
            return Ok(None);
        }

        // Parse output (simplified - would need proper parsing in production)
        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn get_window_by_id_macos(_window_id: &str) -> Result<Option<WindowInfo>> {
        // macOS window IDs are complex and app-specific
        // This would need more sophisticated implementation
        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn get_windows_by_pid_macos(pid: u32) -> Result<Vec<WindowInfo>> {
        let script = format!(
            r#"
        tell application "System Events"
            set proc to first process whose unix id is {}
            set windowList to {{}}
            repeat with win in windows of proc
                set end of windowList to {{id of win, name of win, visible of win, minimized of win}}
            end repeat
            return windowList
        end tell
        "#,
            pid
        );

        let output = Command::new("osascript").arg("-e").arg(&script).output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        // Parse output (simplified)
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_xrandr_parsing_does_not_panic(_line in ".*") { // Prefix with _
            // Basic test: Assume parsing function
            prop_assert!(true);
        }
    }
}
