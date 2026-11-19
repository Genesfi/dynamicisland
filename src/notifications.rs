use crate::island::Notification;
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use windows::core::*;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::*;
use windows::UI::Notifications::*;

/// Cleans an application name by removing packaging junk, stripping underscores, and pretty-formatting.
pub fn clean_app_name(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("whatsapp") {
        return "WhatsApp".to_string();
    } else if lower.contains("discord") {
        return "Discord".to_string();
    } else if lower.contains("telegram") {
        return "Telegram".to_string();
    } else if lower.contains("spotify") {
        return "Spotify".to_string();
    } else if lower.contains("netflix") {
        return "Netflix".to_string();
    } else if lower.contains("youtubemusic") || lower.contains("youtube music") {
        return "YouTube Music".to_string();
    } else if lower.contains("youtube") {
        return "YouTube".to_string();
    } else if lower.contains("chrome") {
        return "Google Chrome".to_string();
    } else if lower.contains("msedge") || lower.contains("edge") {
        return "Microsoft Edge".to_string();
    } else if lower.contains("firefox") {
        return "Mozilla Firefox".to_string();
    } else if lower.contains("outlook") {
        return "Outlook".to_string();
    } else if lower.contains("slack") {
        return "Slack".to_string();
    } else if lower.contains("steam") {
        return "Steam".to_string();
    }

    let mut s = raw.to_string();
    if let Some(pos) = s.rfind('!') {
        s = s[pos + 1..].to_string();
    }
    if let Some(pos) = s.rfind('.') {
        let candidate = &s[pos + 1..];
        if candidate.len() > 2 && !candidate.eq_ignore_ascii_case("exe") {
            s = candidate.to_string();
        }
    }
    if s.ends_with(".exe") || s.ends_with(".EXE") {
        s.truncate(s.len() - 4);
    }
    // Strictly replace all underscores with spaces!
    s = s.replace('_', " ");

    let words: Vec<&str> = s.split_whitespace().collect();
    if words.is_empty() {
        return "Windows".to_string();
    }
    let clean = words
        .iter()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    clean
}

/// Maps an application name to a recognizable single-letter badge and brand color.
pub fn get_app_badge_info(app_name: &str) -> (String, (f32, f32, f32)) {
    let lower = app_name.to_lowercase();

    if lower.contains("whatsapp") {
        ("W".to_string(), (0.15, 0.83, 0.40)) // WhatsApp Emerald Green
    } else if lower.contains("telegram") {
        ("T".to_string(), (0.00, 0.55, 0.88)) // Telegram Vivid Cyan/Blue
    } else if lower.contains("discord") {
        ("D".to_string(), (0.35, 0.40, 0.95)) // Discord Blurple
    } else if lower.contains("spotify") {
        ("S".to_string(), (0.11, 0.73, 0.33)) // Spotify Green
    } else if lower.contains("youtube") {
        ("Y".to_string(), (0.95, 0.10, 0.10)) // YouTube Red
    } else if lower.contains("slack") {
        ("S".to_string(), (0.88, 0.15, 0.45)) // Slack Aubergine/Magenta
    } else if lower.contains("chrome") {
        ("C".to_string(), (0.95, 0.65, 0.05)) // Chrome Warm Amber
    } else if lower.contains("firefox") {
        ("F".to_string(), (1.00, 0.42, 0.10)) // Firefox Vivid Orange
    } else if lower.contains("edge") {
        ("E".to_string(), (0.00, 0.48, 0.85)) // Edge Cobalt Blue
    } else if lower.contains("outlook") || lower.contains("mail") {
        ("M".to_string(), (0.00, 0.45, 0.85)) // Outlook Royal Blue
    } else if lower.contains("github") {
        ("G".to_string(), (0.45, 0.20, 0.75)) // GitHub Purple
    } else if lower.contains("steam") {
        ("S".to_string(), (0.10, 0.25, 0.45)) // Steam Navy Blue
    } else {
        // Fallback: Use the first uppercase character or "•" with iOS Accent Blue
        let letter = app_name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "•".to_string());
        (letter, (0.00, 0.48, 1.00))
    }
}

/// Parses a Windows WinRT `UserNotification` into our clean `Notification` model.
pub fn parse_user_notification(user_notif: &UserNotification) -> Option<Notification> {
    // 1. Extract Display Name of the sender application
    let raw_name = user_notif
        .AppInfo()
        .and_then(|info| info.DisplayInfo())
        .and_then(|disp| disp.DisplayName())
        .map(|h| h.to_string())
        .unwrap_or_else(|_| "Windows".to_string());
    let app_name = clean_app_name(&raw_name);

    // 2. Extract Text Elements (Title & Message Body)
    let notif = user_notif.Notification().ok()?;
    let visual = notif.Visual().ok()?;
    let bindings = visual.Bindings().ok()?;

    let mut title = String::new();
    let mut body = String::new();

    if let Ok(count) = bindings.Size() {
        for i in 0..count {
            if let Ok(binding) = bindings.GetAt(i) {
                if let Ok(texts) = binding.GetTextElements() {
                    if let Ok(text_count) = texts.Size() {
                        if text_count > 0 {
                            if let Ok(t0) = texts.GetAt(0) {
                                if let Ok(txt) = t0.Text() {
                                    title = txt.to_string();
                                }
                            }
                        }
                        if text_count > 1 {
                            if let Ok(t1) = texts.GetAt(1) {
                                if let Ok(txt) = t1.Text() {
                                    body = txt.to_string();
                                }
                            }
                        }
                        if !title.is_empty() {
                            break;
                        }
                    }
                }
            }
        }
    }

    if title.trim().is_empty() && body.trim().is_empty() {
        return None;
    }

    let aumid = user_notif
        .AppInfo()
        .and_then(|info| info.AppUserModelId())
        .map(|h| h.to_string())
        .ok();

    let icon_bytes = extract_logo_bytes(user_notif);

    let (badge_letter, badge_color) = get_app_badge_info(&app_name);

    Some(Notification {
        app_name,
        title,
        body,
        time_str: "now".to_string(),
        badge_letter,
        badge_color,
        icon_bytes,
        aumid,
    })
}

/// Attempts to extract the official high-resolution application logo bytes from WinRT AppDisplayInfo.
fn extract_logo_bytes(user_notif: &UserNotification) -> Option<Vec<u8>> {
    let app_info = user_notif.AppInfo().ok()?;
    let disp = app_info.DisplayInfo().ok()?;
    let size = windows::Foundation::Size { Width: 48.0, Height: 48.0 };
    let stream_ref = disp.GetLogo(size).ok()?;
    let stream_op = stream_ref.OpenReadAsync().ok()?;
    let stream = wait_async(stream_op).ok()?;
    let stream_len = stream.Size().ok()? as usize;
    if stream_len == 0 {
        return None;
    }
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream).ok()?;
    let load_op = reader.LoadAsync(stream_len as u32).ok()?;
    let load_info: windows_future::IAsyncInfo = load_op.cast().ok()?;
    while load_info.Status().ok()? == windows_future::AsyncStatus::Started {
        thread::sleep(Duration::from_millis(10));
    }
    let _ = load_op.GetResults().ok()?;
    let _ = load_info.Close();
    let mut bytes = vec![0u8; stream_len];
    let res = reader.ReadBytes(&mut bytes);
    let _ = reader.Close();
    let _ = stream.Close();
    if res.is_ok() {
        Some(bytes)
    } else {
        None
    }
}

/// Helper to wait on WinRT async operations synchronously on a background thread.
fn wait_async<T: RuntimeType + 'static>(
    op: windows_future::IAsyncOperation<T>,
) -> Result<T> {
    let info: windows_future::IAsyncInfo = op.cast()?;
    while info.Status()? == windows_future::AsyncStatus::Started {
        thread::sleep(Duration::from_millis(20));
    }
    let res = op.GetResults();
    let _ = info.Close();
    res
}

/// Spawns a background thread listening strictly for REAL Windows notifications.
/// When a real notification arrives, it parses it and sends it via `sender`.
pub fn start_notification_listener(sender: Sender<Notification>) {
    thread::spawn(move || {
        let listener = match UserNotificationListener::Current() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[Listener Error] Could not get UserNotificationListener: {:?}", e);
                return;
            }
        };

        let status = match listener.GetAccessStatus() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[Listener Error] Could not query access status: {:?}", e);
                return;
            }
        };

        let status = if status != UserNotificationListenerAccessStatus::Allowed {
            if let Ok(op) = listener.RequestAccessAsync() {
                wait_async(op).unwrap_or(status)
            } else {
                status
            }
        } else {
            status
        };

        if status != UserNotificationListenerAccessStatus::Allowed {
            eprintln!("[Listener Notice] Access status is not Allowed: {:?}", status);
            return;
        }

        // Set to track already processed notification IDs
        let mut seen_ids: HashSet<u32> = HashSet::new();

        // 1. Initial seed: record existing notification IDs so we don't trigger old alerts on startup
        if let Ok(async_op) = listener.GetNotificationsAsync(NotificationKinds::Toast) {
            if let Ok(notifs) = wait_async(async_op) {
                if let Ok(count) = notifs.Size() {
                    for i in 0..count {
                        if let Ok(item) = notifs.GetAt(i) {
                            if let Ok(id) = item.Id() {
                                seen_ids.insert(id);
                            }
                        }
                    }
                }
            }
        }

        // 2. Subscribe to NotificationChanged event for instant 0ms latency
        let sender_event = sender.clone();
        let listener_event = match UserNotificationListener::Current() {
            Ok(l) => l,
            Err(_) => return,
        };

        let event_handler = TypedEventHandler::new(
            move |_sender: Ref<UserNotificationListener>, args: Ref<UserNotificationChangedEventArgs>| {
                if let Some(args_ref) = args.as_ref() {
                    if let Ok(kind) = args_ref.ChangeKind() {
                        if kind == UserNotificationChangedKind::Added {
                            if let Ok(notif_id) = args_ref.UserNotificationId() {
                                if let Ok(user_notif) = listener_event.GetNotification(notif_id) {
                                    if let Some(parsed) = parse_user_notification(&user_notif) {
                                        let _ = sender_event.send(parsed);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            },
        );

        let _ = listener.NotificationChanged(&event_handler);

        // 3. Fallback Polling Loop (checks every 500ms)
        loop {
            thread::sleep(Duration::from_millis(500));

            if seen_ids.len() > 500 {
                seen_ids.clear();
            }

            if let Ok(async_op) = listener.GetNotificationsAsync(NotificationKinds::Toast) {
                if let Ok(notifs) = wait_async(async_op) {
                    if let Ok(count) = notifs.Size() {
                        for i in 0..count {
                            if let Ok(item) = notifs.GetAt(i) {
                                if let Ok(id) = item.Id() {
                                    if seen_ids.insert(id) {
                                        // Real notification detected from Windows!
                                        if let Some(parsed) = parse_user_notification(&item) {
                                            let _ = sender.send(parsed);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Checks the Windows Notification Listener access status.
pub fn check_listener_status() -> Result<String> {
    let listener = UserNotificationListener::Current()?;
    let status = listener.GetAccessStatus()?;

    match status {
        UserNotificationListenerAccessStatus::Allowed => {
            Ok("Allowed: Windows Notification access active".to_string())
        }
        UserNotificationListenerAccessStatus::Denied => {
            Ok("Denied: Check Windows Settings -> Privacy -> Notifications".to_string())
        }
        UserNotificationListenerAccessStatus::Unspecified => {
            Ok("Unspecified: User has not yet prompted permission".to_string())
        }
        _ => Ok("Unknown access status".to_string()),
    }
}
