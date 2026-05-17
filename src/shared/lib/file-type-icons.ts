/**
 * File type icon resolution and rendering.
 *
 * Uses a SVG sprite at `/file-type-icons/sprite.svg` (1000+ icons) with
 * `<svg><use href="#icon-id" />` rendering. Icon IDs are validated against
 * the compile-time set in `file-type-icon-ids.ts`.
 */

import { FILE_TYPE_ICON_IDS } from "@/shared/lib/file-type-icon-ids";

// ---------------------------------------------------------------------------
// Resolution maps (ported from OpenChamber)
// ---------------------------------------------------------------------------

/** Exact filename → icon id */
export const FILE_NAME_ICON_ID_MAP: Record<string, string> = {
  dockerfile: "docker",
  "docker-compose.yml": "docker",
  "docker-compose.yaml": "docker",
  makefile: "makefile",
  gnumakefile: "makefile",
  "cmakelists.txt": "cmake",
  "package.json": "nodejs",
  "package-lock.json": "npm",
  "yarn.lock": "yarn",
  "pnpm-lock.yaml": "pnpm",
  "bun.lock": "bun",
  "bun.lockb": "bun",
  "tsconfig.json": "tsconfig",
  "jsconfig.json": "jsconfig",
  ".gitignore": "git",
  ".gitattributes": "git",
  ".gitmodules": "git",
  ".editorconfig": "editorconfig",
  ".npmrc": "npm",
  ".yarnrc": "yarn",
  ".prettierrc": "prettier",
  ".prettierrc.json": "prettier",
  ".prettierrc.yml": "prettier",
  ".prettierrc.yaml": "prettier",
  ".prettierrc.js": "prettier",
  ".prettierrc.cjs": "prettier",
  ".prettierrc.mjs": "prettier",
  "prettier.config.js": "prettier",
  "prettier.config.cjs": "prettier",
  "prettier.config.mjs": "prettier",
  ".eslintrc": "eslint",
  ".eslintrc.js": "eslint",
  ".eslintrc.cjs": "eslint",
  ".eslintrc.json": "eslint",
  ".eslintrc.yml": "eslint",
  "eslint.config.js": "eslint",
  "eslint.config.mjs": "eslint",
  "eslint.config.cjs": "eslint",
  "eslint.config.ts": "eslint",
  ".babelrc": "babel",
  "babel.config.js": "babel",
  "babel.config.json": "babel",
  "vite.config.ts": "vite",
  "vite.config.js": "vite",
  "vite.config.mjs": "vite",
  "next.config.js": "next",
  "next.config.mjs": "next",
  "next.config.ts": "next",
  "nuxt.config.ts": "nuxt",
  "nuxt.config.js": "nuxt",
  "svelte.config.js": "svelte",
  "tailwind.config.js": "tailwindcss",
  "tailwind.config.ts": "tailwindcss",
  "tailwind.config.cjs": "tailwindcss",
  "tailwind.config.mjs": "tailwindcss",
  "postcss.config.js": "postcss",
  "postcss.config.cjs": "postcss",
  "postcss.config.mjs": "postcss",
  "vitest.config.ts": "vitest",
  "vitest.config.js": "vitest",
  "jest.config.js": "jest",
  "jest.config.ts": "jest",
  "cargo.toml": "rust",
  "cargo.lock": "rust",
  "go.mod": "go",
  "go.sum": "go",
  "requirements.txt": "python",
  "pyproject.toml": "python",
  "setup.py": "python",
  "setup.cfg": "python",
  "gemfile": "ruby",
  "rakefile": "ruby",
  "license": "certificate",
  "license.md": "certificate",
  "license.txt": "certificate",
  "readme.md": "readme",
  "readme": "readme",
  "changelog.md": "changelog",
  "changelog": "changelog",
  ".env": "settings",
  ".env.local": "settings",
  ".env.development": "settings",
  ".env.production": "settings",
  ".env.test": "settings",
  "vercel.json": "vercel",
  "netlify.toml": "netlify",
  "turbo.json": "turborepo",
  "nx.json": "nx",
  "deno.json": "deno",
  "deno.jsonc": "deno",
  "biome.json": "biome",
  "biome.jsonc": "biome",
  ".swcrc": "swc",
  "rollup.config.js": "rollup",
  "rollup.config.ts": "rollup",
  "webpack.config.js": "webpack",
  "webpack.config.ts": "webpack",
  "esbuild.config.js": "esbuild",
  "tsup.config.ts": "settings",
  "components.json": "json",
};

/** File extension → icon id (resolved via language detection) */
export const EXTENSION_ICON_ID_MAP: Record<string, string> = {
  // JavaScript / TypeScript
  js: "javascript",
  jsx: "react",
  mjs: "javascript",
  cjs: "javascript",
  ts: "typescript",
  tsx: "react_ts",
  mts: "typescript",
  cts: "typescript",
  // Web
  html: "html",
  htm: "html",
  css: "css",
  scss: "sass",
  sass: "sass",
  less: "less",
  styl: "stylus",
  vue: "vue",
  svelte: "svelte",
  astro: "astro",
  // Data / config
  json: "json",
  jsonc: "json",
  json5: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  xml: "xml",
  csv: "table",
  ini: "settings",
  env: "settings",
  // Markdown / docs
  md: "markdown",
  mdx: "mdx",
  txt: "document",
  rst: "document",
  // Shell
  sh: "console",
  bash: "console",
  zsh: "console",
  fish: "console",
  ps1: "powershell",
  bat: "console",
  cmd: "console",
  // Languages
  py: "python",
  rb: "ruby",
  rs: "rust",
  go: "go",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  scala: "scala",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  cs: "csharp",
  fs: "fsharp",
  swift: "swift",
  dart: "dart",
  lua: "lua",
  pl: "perl",
  pm: "perl",
  r: "r",
  jl: "julia",
  hs: "haskell",
  ex: "elixir",
  exs: "elixir",
  erl: "erlang",
  clj: "clojure",
  cljs: "clojure",
  ml: "ocaml",
  mli: "ocaml",
  nim: "nim",
  zig: "zig",
  v: "vlang",
  d: "d",
  php: "php",
  // Database
  sql: "database",
  graphql: "graphql",
  gql: "graphql",
  prisma: "prisma",
  // Solidity
  sol: "solidity",
  // Nix
  nix: "nix",
  // Terraform
  tf: "terraform",
  hcl: "terraform",
  // LaTeX
  tex: "tex",
  bib: "bibliography",
  // Protocol / schema
  proto: "proto",
  wasm: "webassembly",
  // Shaders
  glsl: "shader",
  hlsl: "shader",
  // Config
  lock: "lock",
  // Media
  svg: "svg",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  avif: "image",
  bmp: "image",
  ico: "favicon",
  mp3: "audio",
  wav: "audio",
  flac: "audio",
  ogg: "audio",
  mp4: "video",
  mov: "video",
  avi: "video",
  mkv: "video",
  webm: "video",
  pdf: "pdf",
  doc: "word",
  docx: "word",
  ppt: "powerpoint",
  pptx: "powerpoint",
  xls: "table",
  xlsx: "table",
  zip: "zip",
  tgz: "zip",
  gz: "zip",
  rar: "zip",
  "7z": "zip",
  tar: "zip",
};

/** Folder name → icon id (for well-known directories) */
export const FOLDER_NAME_ICON_ID_MAP: Record<string, string> = {
  src: "folder-src",
  lib: "folder-lib",
  dist: "folder-dist",
  build: "folder-dist",
  out: "folder-dist",
  test: "folder-test",
  tests: "folder-test",
  __tests__: "folder-test",
  spec: "folder-test",
  node_modules: "folder-node",
  ".git": "folder-git",
  ".github": "folder-github",
  ".vscode": "folder-vscode",
  public: "folder-public",
  assets: "folder-images",
  images: "folder-images",
  img: "folder-images",
  components: "folder-components",
  hooks: "folder-hook",
  pages: "folder-views",
  views: "folder-views",
  layouts: "folder-layout",
  styles: "folder-css",
  utils: "folder-utils",
  helpers: "folder-helper",
  config: "folder-config",
  docs: "folder-docs",
  scripts: "folder-scripts",
  types: "folder-typescript",
  api: "folder-api",
  middleware: "folder-middleware",
  models: "folder-database",
  services: "folder-server",
  modules: "folder-project",
  features: "folder-project",
  shared: "folder-shared",
  target: "folder-dist",
  ".next": "folder-next",
  ".nuxt": "folder-nuxt",
  migrations: "folder-database",
};

const FALLBACK_ICON = "document";
const FALLBACK_FOLDER_ICON = "folder";

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

function resolveIconId(name: string, isDir: boolean): string {
  const lower = name.toLowerCase();

  if (isDir) {
    // Check folder name map
    if (FOLDER_NAME_ICON_ID_MAP[lower]) {
      const id = FOLDER_NAME_ICON_ID_MAP[lower];
      return FILE_TYPE_ICON_IDS.has(id) ? id : FALLBACK_FOLDER_ICON;
    }
    return FALLBACK_FOLDER_ICON;
  }

  // 1. Exact filename match
  if (FILE_NAME_ICON_ID_MAP[lower]) {
    const id = FILE_NAME_ICON_ID_MAP[lower];
    return FILE_TYPE_ICON_IDS.has(id) ? id : FALLBACK_ICON;
  }

  // 2. .env* pattern
  if (lower.startsWith(".env")) {
    return "settings";
  }

  // 3. Extension match
  const ext = lower.includes(".") ? lower.split(".").pop() ?? "" : "";
  if (ext && EXTENSION_ICON_ID_MAP[ext]) {
    const id = EXTENSION_ICON_ID_MAP[ext];
    return FILE_TYPE_ICON_IDS.has(id) ? id : FALLBACK_ICON;
  }

  // 4. Extension itself might be a valid icon id
  if (ext && FILE_TYPE_ICON_IDS.has(ext)) {
    return ext;
  }

  return FALLBACK_ICON;
}

/**
 * Get the sprite `href` for a file/folder icon.
 * Returns a string like `"/file-type-icons/sprite.svg#typescript"`.
 */
export function getFileTypeIconHref(name: string, isDir: boolean, isExpanded?: boolean): string {
  const id = resolveIconId(name, isDir);

  // For folders, append "-open" when expanded (if the open variant exists in the sprite)
  if (isDir && isExpanded) {
    const openId = id === "folder" ? "folder-open" : `${id}-open`;
    if (FILE_TYPE_ICON_IDS.has(openId)) {
      return `/file-type-icons/sprite.svg#${openId}`;
    }
  }

  return `/file-type-icons/sprite.svg#${id}`;
}
