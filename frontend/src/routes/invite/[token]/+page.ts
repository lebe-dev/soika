// Accept-invite page load: fetch the invite preview so the page can render the
// right flow (join vs. register). Invalid/expired tokens surface via
// the catch in the component, so we pass the token and let the page fetch.

import { invites, errorMessage, type InvitePreview } from '$lib/api';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params, fetch }) => {
  let preview: InvitePreview | null = null;
  let error: string | null = null;
  try {
    preview = await invites.preview(params.token, { fetch });
  } catch (err) {
    error = errorMessage(err, 'This invite is no longer valid.');
  }
  return { token: params.token, preview, error };
};
