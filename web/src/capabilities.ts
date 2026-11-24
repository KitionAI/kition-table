/**
 * What the editor tells its host it does (Kit UI protocol `ui/initialize`):
 *
 * - `ai`: it runs the host's agent steps (`ai/run`);
 * - `replica`: it serves the document's KDM replica (`replica/version`, `replica/export`,
 *   `replica/import`, `replica/info`) and opens with the replica the host keeps. A co-editing
 *   host (the embed's `collaboration`) relays the replica to and from the document's room only
 *   for an editor that declares it: without it the table is never co-edited.
 * - `checkpoints`: it lists the host's checkpoints (section 7m) to tell a record's history before
 *   this session between them.
 */
export const CAPABILITIES = ['ai', 'replica', 'checkpoints']
