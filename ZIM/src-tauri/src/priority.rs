//! Wbudowane tagi ważności (Pilne/Luźne/Zachować). Logika rozpoznaje je po trwałym kluczu `tags.system_key`,
//! więc zmiana nazwy przez użytkownika niczego nie psuje.

pub struct SystemTagDef {
    pub key: &'static str,
    pub name: &'static str,
    pub color: &'static str,
    pub description: &'static str,
}

/// Kolejność = kolejność na liście tagów po pierwszym uruchomieniu
pub const SYSTEM_TAGS: [SystemTagDef; 3] = [
    SystemTagDef {
        key: "urgent",
        name: "Pilne",
        color: "#ef4444",
        description: "wymaga szybkiej reakcji albo decyzji: terminy, prośby od ludzi, ważne sprawy służbowe i osobiste",
    },
    SystemTagDef {
        key: "loose",
        name: "Luźne",
        color: "#3b82f6",
        description: "zwykła korespondencja bez pośpiechu: powiadomienia, newslettery, marketing, automatyczne potwierdzenia",
    },
    SystemTagDef {
        key: "keep",
        name: "Zachować",
        color: "#10b981",
        description: "nie wymaga reakcji, ale warto mieć na później: faktury, rachunki, bilety, potwierdzenia zamówień; trafia od razu do Archiwum",
    },
];

/// Miejsce w Odebranych: 0 = na górze; wiadomości bez tagu ważności lądują na pozycji 2 (przed „keep”)
pub const NO_PRIORITY_RANK: i32 = 2;
pub const INBOX_RANKS: [(&str, i32); 3] = [("urgent", 0), ("loose", 1), ("keep", 3)];
