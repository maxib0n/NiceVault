import { m } from '../paraglide/messages.js';

export const FRAMES = ['guilloche', 'laurel', 'rope', 'meander', 'starburst', 'hairline'] as const;
export type FrameId = (typeof FRAMES)[number];
export const DEFAULT_FRAME: FrameId = 'guilloche';

export function frameOf(id: string | null | undefined): FrameId {
  return (FRAMES as readonly string[]).includes(id ?? '') ? (id as FrameId) : DEFAULT_FRAME;
}

export function frameLabel(id: FrameId): string {
  return {
    guilloche: m.frame_guilloche,
    laurel: m.frame_laurel,
    rope: m.frame_rope,
    meander: m.frame_meander,
    starburst: m.frame_starburst,
    hairline: m.frame_hairline,
  }[id]();
}
