import type { BioStatus } from './types';
import { m } from '../paraglide/messages.js';

export function bioTitle(s: Pick<BioStatus, 'kind'>): string {
  if (s.kind === 'face') return m.bio_title_face();
  if (s.kind === 'fingerprint') return m.bio_title_fingerprint();
  return m.bio_title_biometric();
}

export function bioLabel(s: Pick<BioStatus, 'kind' | 'device'>): string {
  const device = s.device.trim() || 'NiceVault';
  if (s.kind === 'face') return m.bio_label_face({ device });
  if (s.kind === 'fingerprint') return m.bio_label_fingerprint({ device });
  return m.bio_label_biometric({ device });
}

export function bioReason(reason: string): string {
  if (reason === 'none_enrolled') return m.bio_reason_none_enrolled();
  if (reason === 'no_passcode') return m.bio_reason_no_passcode();
  return m.bio_reason_unsupported();
}
