import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Api } from '@shared/api'

/** Zdarzenie z rdzenia -> wywołanie zwrotne; zwraca funkcję wypisującą się (tak jak dawny preload). */
function subscribe<T>(event: string, cb: (payload: T) => void): () => void {
  const unlisten = listen<T>(event, (e) => cb(e.payload))
  return () => void unlisten.then((off) => off())
}

/**
 * Most do rdzenia w Rust. Komenda to dawna nazwa kanału IPC z ':' zamienionym na '_'
 * (`messages:list` -> `messages_list`), a argumenty idą pod nazwami parametrów.
 */
export const api: Api = {
  listMessages: (query) => invoke('messages_list', { query }),
  searchMessages: (query) => invoke('messages_search', { query }),
  getMessage: (id) => invoke('messages_get', { id }),
  markRead: (ids, read) => invoke('messages_read', { ids, read }),
  starMessages: (ids, starred) => invoke('messages_star', { ids, starred }),
  moveMessages: (ids, folder) => invoke('messages_move', { ids, folder }),
  markSpam: (ids) => invoke('messages_spam', { ids }),
  markNotSpam: (ids) => invoke('messages_not_spam', { ids }),
  deleteForever: (ids) => invoke('messages_delete_forever', { ids }),
  listContacts: () => invoke('contacts_list'),
  saveContact: (contact) => invoke('contacts_save', { contact }),
  contactMail: (id) => invoke('contacts_mail', { id }),
  deleteContact: (id) => invoke('contacts_delete', { id }),
  pickContactPhoto: () => invoke('contacts_pick_photo'),
  listTags: () => invoke('tags_list'),
  createTag: (name, color, description) => invoke('tags_create', { name, color, description }),
  updateTag: (id, name, color, description) => invoke('tags_update', { id, name, color, description }),
  getAiStatus: () => invoke('ai_status'),
  setAiConfig: (config) => invoke('ai_set_config', { config }),
  setAiKey: (provider, key) => invoke('ai_set_key', { provider, key }),
  reprocessAi: () => invoke('ai_reprocess'),
  summarizeMessage: (id, force = false) => invoke('messages_summary', { id, force }),
  reclassifyMessage: (id) => invoke('messages_reclassify', { id }),
  onAiTagged: (cb) => subscribe<null>('ai:tagged', () => cb()),
  moveTag: (id, direction) => invoke('tags_move', { id, direction }),
  deleteTag: (id) => invoke('tags_delete', { id }),
  tagMessages: (ids, tagId, on) => invoke('messages_tag', { ids, tagId, on }),
  openAttachment: (id) => invoke('attachments_open', { id }),
  saveAttachment: (id) => invoke('attachments_save', { id }),
  saveTextFile: (filename, content) => invoke('files_save_text', { filename, content }),
  pickFiles: () => invoke('compose_pick_files'),
  sendMail: (draft) => invoke('compose_send', { draft }),
  scheduleMail: (draft, sendAt) => invoke('compose_schedule', { draft, sendAt }),
  listScheduled: (accountId) => invoke('outbox_list', { accountId }),
  cancelScheduled: (id) => invoke('outbox_cancel', { id }),
  retryScheduled: (id) => invoke('outbox_retry', { id }),
  onOutboxChanged: (cb) => subscribe<null>('outbox:changed', () => cb()),
  listAccounts: () => invoke('accounts_list'),
  addAccount: (input) => invoke('accounts_add', { input }),
  setAccountAlias: (id, alias) => invoke('accounts_set_alias', { id, alias }),
  removeAccount: (id) => invoke('accounts_remove', { id }),
  onSyncStatus: (cb) => subscribe('sync:status', cb)
}
