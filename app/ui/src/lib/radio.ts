// Arrow keys inside a role=radiogroup of role=radio buttons (segmented controls, swatches):
// move the choice and the focus together, wrapping. Tab moves between groups.
export function radioKeys<T>(e: KeyboardEvent, values: readonly T[], current: T, set: (v: T) => void): void {
  const i = values.indexOf(current);
  let j = -1;
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') j = (i + 1) % values.length;
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') j = (i - 1 + values.length) % values.length;
  if (j < 0) return;
  e.preventDefault();
  set(values[j]);
  const group = (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="radio"]');
  group[j]?.focus();
}
