# lumenprotocol.org

Zola site (built with zola 0.23.x). `content/spec/v0` is a symlink to `../../../spec/v0`, so the published spec is the repo's `spec/v0/` directly.

Local build:

    zola build && npx -y pagefind --site public

Cloudflare Pages (root directory `site`) does not rely on the symlink. Its build command replaces it with a copy first:

    rm -rf content/spec/v0 && mkdir -p content/spec && cp -R ../spec/v0 content/spec/v0 && zola build && npx -y pagefind@1.5.2 --site public

Security headers (strict CSP) are in `static/_headers`.
