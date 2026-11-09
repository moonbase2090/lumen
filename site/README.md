# lumenprotocol.org

Zola site (built with zola 0.23.x). `content/spec/v0` is a symlink to `../../../spec/v0`, so the published spec is the repo's `spec/v0/` directly.

Local build:

    zola build && npx -y pagefind --site public

Cloudflare Pages (root directory `site`) does not rely on the symlink. Its build command replaces it with a copy first:

    rm -rf content/spec/v0 && mkdir -p content/spec && cp -R ../spec/v0 content/spec/v0 && zola build && npx -y pagefind@1.5.2 --site public

Security headers (strict CSP) are in `static/_headers`.

## Deploying

The site is static. Cloudflare Pages builds it from this directory: the project root is `site`, the build command is the one above (it swaps the symlink for a copy of `spec/v0`, runs `zola build`, then indexes with Pagefind), and the output directory is `public`. Nothing in the repository holds a token, a zone or an account setting; those live in the Pages project, not here.

The `site-build` check in CI runs the same steps on every pull request and on pushes to `main`, with the same pinned Zola and Pagefind versions. It fails on a build error, on a Pagefind warning, on a CSS or script link in `templates/base.html` without `cachebust=true`, on text that the public-text scan refuses (including in the built pages), and on an internal link that does not resolve. It does not publish anything and it does not check the live site, the headers as served, or the external links (a separate scheduled check covers those).

Publishing is a separate decision from merging. While the Pages Git integration publishes from `main`, a merge that touches `site/` or `spec/` is a publish, so such a merge waits for an explicit go.

To preview before that, build as above and serve the result, or run `zola serve` in this directory.
