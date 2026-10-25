// Backend error codes -> one short sentence in the UI language. The backend's own message
// (English) is only a fallback for codes the UI does not know.
import type { ApiError } from './types';
import { isApiError } from './api';
import { m } from '../paraglide/messages.js';

const TEXT: Record<string, () => string> = {
  wrong_password: m.err_wrong_password,
  not_found: m.err_not_found,
  exists: m.err_exists,
  not_empty: m.err_not_empty,
  not_dir: m.err_not_dir,
  is_dir: m.err_is_dir,
  invalid_name: m.err_invalid_name,
  locked: m.err_locked,
  corrupt: m.err_corrupt,
  io: m.err_io,
  autostart_failed: m.err_autostart_failed,
  last_slot: m.err_last_slot,
  slots_full: m.err_slots_full,
  mount_unavailable: m.err_mount_unavailable,
  mount_failed: m.err_mount_failed,
  busy: m.err_busy,
  photos_failed: m.err_photos_failed,
  expired: m.err_expired,
  cancelled: m.err_cancelled,
  not_a_vault: m.err_not_a_vault,
  invalid_password: m.err_invalid_password,
  invalid_slot: m.err_invalid_slot,
  duress_conflict: m.err_duress_conflict,
  open_failed: m.err_open_failed,
  rolled_back: m.err_rolled_back,
  unsafe_type: m.err_unsafe_type,
  invalid_emblem: m.err_invalid_emblem,
  invalid_appearance: m.err_invalid_appearance,
  // search + tags (search.rs)
  invalid_color: m.err_invalid_color,
  too_many_tags: m.err_too_many_tags,
  invalid_query: m.err_invalid_query,
  // password manager (pm_*)
  invalid_entry: m.err_invalid_entry,
  invalid_totp: m.err_invalid_totp,
  invalid_field: m.err_invalid_field,
  invalid_options: m.err_invalid_options,
  invalid_import: m.err_invalid_import,
  unsupported: m.err_unsupported,
  clipboard: m.err_clipboard,
  unsupported_version: m.err_unsupported_version,
  // updater
  update_network: m.err_update_network,
  update_signature: m.err_update_signature,
  update_none: m.err_update_none,
  update_invalid: m.err_update_invalid,
  update_package: m.err_update_package,
  update_install: m.err_update_install,
  update_restart: m.err_update_restart,
  // Crea dispositivo (device/)
  device_not_found: m.err_device_not_found,
  device_refused: m.err_device_refused,
  device_changed: m.err_device_changed,
  device_busy: m.err_device_busy,
  device_gone: m.err_device_gone,
  device_download: m.err_device_download,
  elevation_denied: m.err_elevation_denied,
  elevation_unavailable: m.err_elevation_unavailable,
  fs_unsupported: m.err_fs_unsupported,
  partition_failed: m.err_partition_failed,
  format_failed: m.err_format_failed,
  client_missing: m.err_client_missing,
  signature_invalid: m.err_signature_invalid,
  copy_failed: m.err_copy_failed,
  confirmation_mismatch: m.err_confirmation_mismatch,
  no_space: m.err_no_space,
  // Aggiorna i programmi sulla chiavetta (device/stick.rs)
  stick_not_mounted: m.err_stick_not_mounted,
  stick_not_device: m.err_stick_not_device,
  stick_full: m.err_stick_full,
  // Aggiorna formato on a nearly full fixed-size volume
  volume_full: m.err_volume_full,
  // a delete job on a full volume / drive (delete.rs)
  no_room_to_delete: m.err_no_room_to_delete,
  invalid_size: m.err_invalid_size,
  // in-app viewers (viewers.rs)
  not_mounted: m.err_not_mounted,
  bad_link: m.err_bad_link,
  not_media: m.err_not_media,
  not_a_file: m.err_not_a_file,
  media_server: m.err_media_server,
  // raised by the UI itself (emblem.ts)
  image_unreadable: m.err_image_unreadable,
  image_complex: m.err_image_complex,
};

/** The sentence for an error code, or null when the UI has none. */
export function errorCodeText(code: string): string | null {
  return TEXT[code]?.() ?? null;
}

/** Known codes get a fixed, translated sentence; unknown ones keep the backend's message. */
export function errorText(e: unknown): string {
  if (isApiError(e)) {
    const err = e as ApiError;
    return errorCodeText(err.code) ?? (err.message?.trim() || m.err_generic());
  }
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return m.err_generic();
}

export function errorCode(e: unknown): string | null {
  return isApiError(e) ? (e as ApiError).code : null;
}
