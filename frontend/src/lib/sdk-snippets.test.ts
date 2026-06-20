import { describe, expect, it } from 'vitest';
import { sdkSnippets } from './sdk-snippets';

const DSN = 'https://abc123@errors.example.com/p42';
const KEY = 'abc123';

describe('sdkSnippets', () => {
  it('covers all languages and embeds the DSN in each', () => {
    const snippets = sdkSnippets(DSN, KEY);
    const langs = snippets.map((s) => s.language);
    expect(langs).toEqual(['go', 'rust', 'javascript', 'python', 'java', 'kotlin', 'generic']);
    for (const snippet of snippets) {
      expect(snippet.code, `${snippet.language} missing DSN`).toContain(DSN);
    }
  });

  it('documents both ingestion endpoints in the generic snippet', () => {
    const generic = sdkSnippets(DSN, KEY).find((s) => s.language === 'generic');
    expect(generic?.code).toContain('https://errors.example.com/api/p42/envelope/');
    expect(generic?.code).toContain('https://errors.example.com/api/p42/store/');
    expect(generic?.code).toContain(`sentry_key=${KEY}`);
  });

  it('derives ingestion URLs from a DSN with a custom port', () => {
    const generic = sdkSnippets('http://k@localhost:8080/p1', 'k').find(
      (s) => s.language === 'generic'
    );
    expect(generic?.code).toContain('http://localhost:8080/api/p1/envelope/');
    expect(generic?.code).toContain('http://localhost:8080/api/p1/store/');
  });
});
