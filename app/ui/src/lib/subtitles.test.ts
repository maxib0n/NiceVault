import { expect, test } from 'vitest';
import { splitSub, srtToVtt } from './subtitles';

test('srt to vtt', () => {
  expect(srtToVtt('1\r\n00:00:01,500 --> 00:00:02,000\r\nCiao\r\n')).toBe('WEBVTT\n\n1\n00:00:01.500 --> 00:00:02.000\nCiao\n');
});
test('sidecar names', () => {
  expect(splitSub('film.it.srt')).toEqual({ base: 'film', lang: 'it', ext: 'srt' });
  expect(splitSub('film.vtt')).toEqual({ base: 'film', lang: '', ext: 'vtt' });
  expect(splitSub('film.mkv')).toBe(null);
});
