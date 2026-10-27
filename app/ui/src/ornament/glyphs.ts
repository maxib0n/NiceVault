import {
  Baby,
  BookOpen,
  Briefcase,
  Camera,
  Car,
  FileText,
  Gem,
  Gift,
  GraduationCap,
  Heart,
  HeartPulse,
  House,
  KeyRound,
  Landmark,
  Leaf,
  Mountain,
  PawPrint,
  PiggyBank,
  Plane,
  Receipt,
  Shield,
  Star,
  Users,
  Wallet,
} from '@lucide/svelte';
import type { Component } from 'svelte';
import type { EmblemIconId } from '../lib/emblem';

export const EMBLEM_GLYPHS: Record<EmblemIconId, Component<{ size?: number | string; 'aria-hidden'?: boolean | 'true' }>> = {
  house: House,
  'file-text': FileText,
  camera: Camera,
  briefcase: Briefcase,
  users: Users,
  'heart-pulse': HeartPulse,
  car: Car,
  plane: Plane,
  wallet: Wallet,
  'piggy-bank': PiggyBank,
  landmark: Landmark,
  receipt: Receipt,
  'key-round': KeyRound,
  heart: Heart,
  star: Star,
  'graduation-cap': GraduationCap,
  baby: Baby,
  'paw-print': PawPrint,
  'book-open': BookOpen,
  gift: Gift,
  shield: Shield,
  gem: Gem,
  leaf: Leaf,
  mountain: Mountain,
};

export function glyphCentre(svg: SVGSVGElement): { x: number; y: number } {
  try {
    const b = svg.getBBox();
    if (b.width > 0 && b.height > 0) return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
  } catch {
  }
  return { x: 12, y: 12 };
}

export function glyphPath(svg: SVGSVGElement): Path2D {
  const p = new Path2D();
  const n = (el: Element, a: string) => Number(el.getAttribute(a) ?? 0);
  for (const el of svg.querySelectorAll('path, circle, ellipse, rect, line, polyline, polygon')) {
    switch (el.tagName.toLowerCase()) {
      case 'path':
        p.addPath(new Path2D(el.getAttribute('d') ?? ''));
        break;
      case 'circle':
        p.moveTo(n(el, 'cx') + n(el, 'r'), n(el, 'cy'));
        p.arc(n(el, 'cx'), n(el, 'cy'), n(el, 'r'), 0, Math.PI * 2);
        break;
      case 'ellipse':
        p.moveTo(n(el, 'cx') + n(el, 'rx'), n(el, 'cy'));
        p.ellipse(n(el, 'cx'), n(el, 'cy'), n(el, 'rx'), n(el, 'ry'), 0, 0, Math.PI * 2);
        break;
      case 'rect':
        p.roundRect(n(el, 'x'), n(el, 'y'), n(el, 'width'), n(el, 'height'), n(el, 'rx'));
        break;
      case 'line':
        p.moveTo(n(el, 'x1'), n(el, 'y1'));
        p.lineTo(n(el, 'x2'), n(el, 'y2'));
        break;
      default: {
        const pts = (el.getAttribute('points') ?? '').trim().split(/[\s,]+/).map(Number);
        for (let i = 0; i + 1 < pts.length; i += 2) {
          if (i === 0) p.moveTo(pts[i], pts[i + 1]);
          else p.lineTo(pts[i], pts[i + 1]);
        }
        if (el.tagName.toLowerCase() === 'polygon') p.closePath();
      }
    }
  }
  return p;
}
