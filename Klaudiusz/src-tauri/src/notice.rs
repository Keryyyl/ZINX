//! Powiadomienia macOS przez UserNotifications. Klaudiusz sam prosi o zgodę (okno systemowe),
//! a powiadomienia pokazują się pod jego nazwą, a nie jako „Script Editor”.

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
        UNUserNotificationCenter,
    };

    /// Pokazuje systemowe pytanie o zgodę na powiadomienia (raz; potem decyduje Ustawienia systemowe).
    pub fn request_permission() {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
        let handler = RcBlock::new(|granted: Bool, _error: *mut NSError| {
            eprintln!("Powiadomienia: zgoda = {}", granted.as_bool());
        });
        center.requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    pub fn send(text: &str) -> Result<(), String> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str("Klaudiusz"));
        content.setBody(&NSString::from_str(text));
        let sound = UNNotificationSound::defaultSound();
        content.setSound(Some(&sound));
        let id = NSString::from_str(&format!("klaudiusz-{}", crate::dates::now_ms()));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.addNotificationRequest_withCompletionHandler(&request, None);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use imp::{request_permission, send};

#[cfg(not(target_os = "macos"))]
pub fn request_permission() {}

#[cfg(not(target_os = "macos"))]
pub fn send(_text: &str) -> Result<(), String> {
    Ok(())
}
