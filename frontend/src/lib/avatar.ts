// Avatar helpers shared by every identity chip (teams, projects, people):
// initials from the first and last word, and a deterministic soft tint per id
// so a roster reads at a glance without being loud.

export const AVATAR_TINTS = [
  'bg-primary/10 text-primary',
  'bg-pink-500/10 text-pink-600 dark:text-pink-400',
  'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
  'bg-amber-500/10 text-amber-600 dark:text-amber-500',
  'bg-violet-500/10 text-violet-600 dark:text-violet-400',
  'bg-cyan-500/10 text-cyan-600 dark:text-cyan-400'
] as const;

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return [...parts[0]].slice(0, 2).join('').toUpperCase();
  return ([...parts[0]][0] + [...parts.at(-1)!][0]).toUpperCase();
}

export function avatarTint(id: string): string {
  let h = 0;
  for (const character of id) h = (h * 31 + character.codePointAt(0)!) >>> 0;
  return AVATAR_TINTS[h % AVATAR_TINTS.length];
}
