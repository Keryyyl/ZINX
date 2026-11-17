//! Odczyt urodzin z książki kontaktów. Na macOS czytamy Apple Contacts (kontakty z iPhone'a
//! trafiają tu przez iCloud). Na innych systemach zwracamy wyraźny komunikat.

use crate::birthdays::Birthday;

#[cfg(target_os = "macos")]
mod macos {
    use super::Birthday;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2_contacts::{
        CNAuthorizationStatus, CNContact, CNContactBirthdayKey, CNContactFamilyNameKey, CNContactFetchRequest, CNContactGivenNameKey,
        CNContactIdentifierKey, CNContactStore, CNEntityType, CNKeyDescriptor,
    };
    use objc2_foundation::{NSArray, NSError, NSString};
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    const NO_ACCESS: &str = "Brak dostępu do kontaktów. Zezwól CTHX w Ustawieniach systemowych → Prywatność i ochrona → Kontakty.";

    /// Prosi o dostęp (pierwszy raz pokazuje okno systemowe) i czyta urodziny. Blokuje wątek,
    /// więc wywołujemy to z `spawn_blocking`, nigdy z głównego wątku.
    pub fn birthdays() -> Result<Vec<Birthday>, String> {
        let store = unsafe { CNContactStore::new() };
        let status = unsafe { CNContactStore::authorizationStatusForEntityType(CNEntityType::Contacts) };

        if status == CNAuthorizationStatus::NotDetermined {
            let (tx, rx) = mpsc::channel::<bool>();
            let block = RcBlock::new(move |granted: Bool, _err: *mut NSError| {
                let _ = tx.send(granted.as_bool());
            });
            unsafe { store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &block) };
            let granted = rx.recv_timeout(Duration::from_secs(120)).unwrap_or(false);
            if !granted {
                return Err(NO_ACCESS.into());
            }
        } else if status != CNAuthorizationStatus::Authorized && status != CNAuthorizationStatus::Limited {
            return Err(NO_ACCESS.into());
        }

        let keys_owned = unsafe { [CNContactIdentifierKey, CNContactGivenNameKey, CNContactFamilyNameKey, CNContactBirthdayKey] };
        let keys: Vec<&ProtocolObject<dyn CNKeyDescriptor>> = keys_owned.iter().map(|k| ProtocolObject::from_ref(*k)).collect();
        let keys_array = NSArray::from_slice(&keys);
        let request = unsafe { CNContactFetchRequest::initWithKeysToFetch(CNContactFetchRequest::alloc(), &keys_array) };

        let found: Rc<RefCell<Vec<Birthday>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = found.clone();
        let block = RcBlock::new(move |contact: NonNull<CNContact>, _stop: NonNull<Bool>| {
            let c = unsafe { contact.as_ref() };
            let Some(date) = (unsafe { c.birthday() }) else { return };
            let (month, day, year) = (date.month(), date.day(), date.year());
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return;
            }
            let given = text(unsafe { c.givenName() });
            let family = text(unsafe { c.familyName() });
            let name = [given, family].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
            sink.borrow_mut().push(Birthday {
                contact_id: text(unsafe { c.identifier() }),
                name: if name.is_empty() { "Bez nazwy".into() } else { name },
                month: month as u32,
                day: day as u32,
                year: (1..=9999).contains(&year).then_some(year as i32),
            });
        });

        let mut error: Option<Retained<NSError>> = None;
        let ok = unsafe { store.enumerateContactsWithFetchRequest_error_usingBlock(&request, Some(&mut error), &block) };
        if !ok {
            return Err("Nie udało się odczytać kontaktów".into());
        }
        Ok(found.take())
    }

    fn text(value: Retained<NSString>) -> String {
        value.to_string().trim().to_string()
    }
}

#[cfg(target_os = "macos")]
pub fn birthdays() -> Result<Vec<Birthday>, String> {
    macos::birthdays()
}

#[cfg(not(target_os = "macos"))]
pub fn birthdays() -> Result<Vec<Birthday>, String> {
    Err("Synchronizacja urodzin z kontaktów działa na razie tylko na macOS.".into())
}
