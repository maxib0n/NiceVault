import type { FsNode } from './types';
import { m } from '../paraglide/messages.js';

export type FileType =
  | 'dir'
  | 'image'
  | 'video'
  | 'audio'
  | 'pdf'
  | 'text'
  | 'archive'
  | 'document'
  | 'spreadsheet'
  | 'presentation'
  | 'key'
  | 'other';

const MAP: Record<string, FileType> = {};
const reg = (t: FileType, exts: string) => exts.split(' ').forEach((e) => (MAP[e] = t));
reg('image', 'jpg jpeg png gif webp avif bmp svg heic');
reg('video', 'mp4 webm mov mkv m4v avi');
reg('audio', 'mp3 ogg oga opus wav flac m4a aac');
reg('pdf', 'pdf');
reg('text', 'txt md csv json log ini conf yaml yml toml xml html css js ts rs py sh');
reg('archive', 'zip gz tgz tar 7z rar xz bz2 zst');
reg('document', 'doc docx odt rtf pages');
reg('spreadsheet', 'xls xlsx ods numbers');
reg('presentation', 'ppt pptx odp key');
reg('key', 'pub pem key asc gpg kdbx nvkey');

export function extension(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

export function fileType(node: Pick<FsNode, 'name' | 'kind'>): FileType {
  if (node.kind === 'dir') return 'dir';
  const ext = extension(node.name);
  if (!ext && /^id_[a-z0-9]+$/i.test(node.name)) return 'key';
  return MAP[ext] ?? 'other';
}

export const PREVIEWABLE: ReadonlySet<FileType> = new Set(['image', 'video', 'audio', 'pdf', 'text', 'key', 'archive', 'document', 'spreadsheet', 'presentation', 'other']);

export function canPreview(node: Pick<FsNode, 'name' | 'kind'>): boolean {
  return PREVIEWABLE.has(fileType(node));
}

const TYPE_TEXT: Record<FileType, () => string> = {
  dir: m.type_dir,
  image: m.type_image,
  video: m.type_video,
  audio: m.type_audio,
  pdf: m.type_pdf,
  text: m.type_text,
  archive: m.type_archive,
  document: m.type_document,
  spreadsheet: m.type_spreadsheet,
  presentation: m.type_presentation,
  key: m.type_key,
  other: m.type_other,
};

export const typeLabel = (t: FileType) => TYPE_TEXT[t]();
