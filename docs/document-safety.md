# Document safety rules

What HushHour guarantees about the user's text, and how. Every port must keep these guarantees,
whatever its UI or storage layer. The Linux implementation is in `src/main.rs`; the headless
tests named below are the executable form of each rule and should be reproduced on every platform.

## Terms

- **Document**: the text in the editor. **Saved text**: a snapshot of the text as last loaded or
  saved. **Dirty**: the text differs from the saved text. Dirty is recomputed from that comparison
  after every edit, so undoing back to the saved revision makes the document clean again.
- **Generation**: a counter bumped whenever the document is replaced (New, Open). **Edits**: a
  counter bumped on every edit, undo and redo. Together they identify one exact revision.
- **Request**: every Open (chooser or direct read) gets an increasing id; only the latest
  outstanding request may deliver a result.

## Rules

1. **Nothing replaces unsaved text without the user's answer for that exact revision.**
   New, Open, a recent file, and Quit all ask when dirty. "Don't save" for an Open records
   `(request id, generation, edits)`. When the file arrives, it replaces the document only if all
   three still match. If the user typed meanwhile, the loaded text is parked and the question is
   asked again (Replace). Cancel on that dialog keeps the newer text and drops the loaded file.
   Tests: `edits_after_discard_revoke_the_replacement`, `discard_without_further_edits_replaces_once`,
   `cancelled_discard_does_not_authorise_an_older_load`, `only_the_latest_open_request_can_replace`.
2. **A save result applies only to the document it was taken from.** A save snapshots the text and
   the generation. On completion, if the generation changed, the result is ignored. Otherwise the
   saved text becomes that snapshot and dirty is recomputed: text typed during the save leaves the
   document dirty ("Saved (edited since)"), never silently marked clean.
3. **One save in flight.** A second Save while one is running is refused with a notice, not queued
   blindly. "Save, then quit/new/open" arms a pending action that runs only if the save succeeds
   and the document is still clean afterwards; if it is dirty again, the question is asked again.
   Cancel on the dialog disarms any pending action. A failed or cancelled save drops the pending
   action. Tests: `cancel_disarms_a_pending_transition`, `typing_during_a_save_starts_a_new_undo_step`.
4. **Saves are atomic and never refused for size.** Write to an exclusively created private
   temporary file next to the target, flush, copy permissions, rename over the target, sync the
   directory (best effort). On any failure the original is untouched; there is no fallback to
   writing in place. A document the editor allowed is always within the size limit, so the writer
   never refuses it. Tests: `atomic::tests::*`.
5. **The size limit is enforced where text grows, inclusively, and matches Open.** Every edit
   that adds bytes (typing, Enter, paste) is judged on the result with the selection subtracted;
   deletions are never refused. Open rejects files above the limit (20 MiB) and reads a bounded
   number of bytes regardless of reported size. So what was saved reopens.
   Tests: `edits_cannot_grow_the_document_past_the_limit`, `the_open_limit_is_inclusive`.
6. **Undo never crosses a save boundary silently.** The typing group ends when a save is
   requested (Save and Save As alike) and when it completes, so edits made during a save undo
   separately. History is bounded (2000 steps, 64 MiB of delta text); the newest step is never
   dropped. Tests: `typing_during_a_save_as_starts_a_new_undo_step`, `coalescing_respects_the_byte_budget`.
7. **Late results are ignored after the document moved on.** Open results from superseded
   requests, save results from an older generation, and anything arriving after quitting began
   are dropped. Loading a document resets the outstanding request and any authorisation.
8. **One exit path.** Every way out (Quit, close button, Quit after "Don't save", Quit after a
   save) runs the same cleanup. Test: `clean_quit_runs_the_cleanup_path`.
9. **State files are written atomically, read bounded, and separated from memory.** Preferences
   and the recent list use the same atomic writer; reading them is capped (64 KiB). Updating the
   in-memory recent list is separate from persisting it, so tests and diagnostic runs write
   nothing. Test: `recent_list_is_written_only_when_persisting`.
10. **Failures are told, never hidden.** A recent-list write failure is shown alongside a
    successful save, not instead of it. Test: `save_notice_keeps_the_recent_list_error`.

## Autosave (for ports that have it)

Autosave is just a save request and obeys rules 2, 3 and 6: snapshot text and generation, apply
the result only if the generation matches, recompute dirty, end the typing group. Beyond that:

- Autosave only ever writes to the file the user already chose; it never picks a path.
- Rule 1 still holds in spirit: New, Open and Close must never discard text that is not yet
  persisted. With autosave the question may become "wait for the pending save" rather than a
  dialog, but a failed autosave must surface and must block a destructive transition.
- External changes to the file (iCloud, another device) are a conflict, not an overwrite: keep
  the user's revision and offer the choice, as `UIDocument` versioning does.
- Keep the counters: revision identity is what makes every rule above checkable.
