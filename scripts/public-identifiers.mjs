// Published reviewer identity in AGENTS.md; it is not a private machine address.
// Keep this exact, never derive an allowlist from untrusted repository content.
export function withoutPublicIdentifiers(text) {
  return text.replace(/(?<![\w.@:/-])helios-codex(?![\w.@:/-])/gi, '');
}

export function overlayContains(text, value) {
  const publicReviewer = 'helios-codex';
  const hostname = publicReviewer.split('-')[0];
  const input = value.toLowerCase() === hostname ? withoutPublicIdentifiers(text) : text;
  return input.toLowerCase().includes(value.toLowerCase());
}
