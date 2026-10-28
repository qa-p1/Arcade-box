# Webpage to Markdown / Text (`arcade.web.markdown`)

**Input:** one HTTP(S) URL. **Output:** Markdown text. **Privacy:** NETWORK.

Fetches static HTML (up to 5 MiB), removes script/style/template content, and extracts common headings, paragraphs, lists, links, and code. Links are retained as references and sensitive query parameters are redacted. It does not render JavaScript or promise article-quality extraction from arbitrary page layouts.
