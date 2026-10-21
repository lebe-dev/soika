// Display-name rule, mirrored from the backend (`normalize_display_name`):
// at least MIN_DISPLAY_NAME_LENGTH characters after trimming. The API enforces
// it too; checking here lets forms explain the rule before submitting.

export const MIN_DISPLAY_NAME_LENGTH = 3;

/** Returns a user-facing error for an invalid display name, or null if valid. */
export function displayNameError(name: string): string | null {
  // Spread by code point so the count matches Rust's `chars().count()`.
  if ([...name.trim()].length >= MIN_DISPLAY_NAME_LENGTH) return null;
  return `Display name must be at least ${MIN_DISPLAY_NAME_LENGTH} characters`;
}
