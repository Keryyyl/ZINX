import type { CloudProvider } from './ai'
import type { ContactInit, ContactMail } from './contact'
import type {
  Account,
  AiConfig,
  AiStatus,
  Folder,
  MessageDetail,
  MessageQuery,
  MessageRow,
  OutgoingDraft,
  PickedFile,
  ScheduledMail,
  SyncStatus,
  Tag
} from './types'

export type NewAccountInput = Omit<Account, 'id' | 'smtpHost' | 'smtpPort'> &
  Partial<Pick<Account, 'smtpHost' | 'smtpPort'>> & { password: string }

/** Kontrakt między interfejsem a rdzeniem (Rust). Ten sam co dawny `window.api` z Electrona. */
export interface Api {
  listMessages(query: MessageQuery): Promise<MessageRow[]>
  searchMessages(query: { accountId: number | null; text: string }): Promise<MessageRow[]>
  getMessage(id: number): Promise<MessageDetail | null>
  markRead(ids: number[], read: boolean): Promise<void>
  starMessages(ids: number[], starred: boolean): Promise<void>
  moveMessages(ids: number[], folder: Folder): Promise<void>
  /** Zwraca liczbę zablokowanych nadawców */
  markSpam(ids: number[]): Promise<number>
  markNotSpam(ids: number[]): Promise<void>
  deleteForever(ids: number[]): Promise<void>
  listContacts(): Promise<ContactInit[]>
  saveContact(contact: ContactInit): Promise<ContactInit>
  contactMail(id: number): Promise<ContactMail[]>
  deleteContact(id: number): Promise<void>
  /** Okno wyboru zdjęcia; zwraca data URL albo null po anulowaniu */
  pickContactPhoto(): Promise<string | null>
  listTags(): Promise<Tag[]>
  createTag(name: string, color: string | null, description: string): Promise<Tag>
  updateTag(id: number, name: string, color: string | null, description: string): Promise<Tag>
  getAiStatus(): Promise<AiStatus>
  /** `consent` dołącza interfejs dopiero po zaznaczeniu zgody na wysyłanie treści maili do dostawcy w chmurze */
  setAiConfig(config: AiConfig & { consent?: boolean }): Promise<void>
  /** null usuwa klucz; klucza nie da się odczytać z powrotem */
  setAiKey(provider: CloudProvider, key: string | null): Promise<void>
  reprocessAi(): Promise<void>
  summarizeMessage(id: number, force?: boolean): Promise<string | null>
  /** Tagi od AI dla jednej wiadomości od razu; zwraca ją z nowymi tagami */
  reclassifyMessage(id: number): Promise<MessageDetail | null>
  onAiTagged(cb: () => void): () => void
  moveTag(id: number, direction: 'up' | 'down'): Promise<void>
  deleteTag(id: number): Promise<void>
  /** Tylko istniejące tagi; nowe powstają wyłącznie w ustawieniach */
  tagMessages(ids: number[], tagId: number, on: boolean): Promise<void>
  openAttachment(id: number): Promise<void>
  /** false = użytkownik anulował wybór miejsca zapisu */
  saveAttachment(id: number): Promise<boolean>
  /** Natywne okno „Zapisz jako”; false = użytkownik anulował */
  saveTextFile(filename: string, content: string): Promise<boolean>
  pickFiles(): Promise<PickedFile[]>
  sendMail(draft: OutgoingDraft): Promise<void>
  scheduleMail(draft: OutgoingDraft, sendAt: number): Promise<number>
  listScheduled(accountId: number | null): Promise<ScheduledMail[]>
  cancelScheduled(id: number): Promise<void>
  retryScheduled(id: number): Promise<void>
  onOutboxChanged(cb: () => void): () => void
  listAccounts(): Promise<Account[]>
  addAccount(input: NewAccountInput): Promise<Account>
  setAccountAlias(id: number, alias: string | null): Promise<string | null>
  removeAccount(id: number): Promise<void>
  onSyncStatus(cb: (s: SyncStatus) => void): () => void
}
