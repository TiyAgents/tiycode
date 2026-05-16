import { describe, expect, it } from "vitest";
import { FILE_TYPE_ICON_IDS } from "@/shared/lib/file-type-icon-ids";
import {
  EXTENSION_ICON_ID_MAP,
  FILE_NAME_ICON_ID_MAP,
  FOLDER_NAME_ICON_ID_MAP,
  getFileTypeIconHref,
} from "@/shared/lib/file-type-icons";

const iconMaps = {
  fileNames: FILE_NAME_ICON_ID_MAP,
  extensions: EXTENSION_ICON_ID_MAP,
  folders: FOLDER_NAME_ICON_ID_MAP,
};

describe("file type icon mappings", () => {
  it("only references sprite IDs that exist", () => {
    const missing = Object.entries(iconMaps).flatMap(([mapName, map]) =>
      Object.entries(map)
        .filter(([, iconId]) => !FILE_TYPE_ICON_IDS.has(iconId))
        .map(([key, iconId]) => `${mapName}.${key} -> ${iconId}`),
    );

    expect(missing).toEqual([]);
  });
});

describe("getFileTypeIconHref", () => {
  it("resolves exact filename mappings case-insensitively", () => {
    expect(getFileTypeIconHref("package-lock.json", false)).toBe(
      "/file-type-icons/sprite.svg#npm",
    );
    expect(getFileTypeIconHref("Dockerfile", false)).toBe(
      "/file-type-icons/sprite.svg#docker",
    );
  });

  it("uses the settings icon for .env-prefixed files", () => {
    expect(getFileTypeIconHref(".env.local", false)).toBe(
      "/file-type-icons/sprite.svg#settings",
    );
    expect(getFileTypeIconHref(".ENV.PRODUCTION", false)).toBe(
      "/file-type-icons/sprite.svg#settings",
    );
    expect(getFileTypeIconHref(".env.custom", false)).toBe(
      "/file-type-icons/sprite.svg#settings",
    );
  });

  it("resolves known extensions and uppercase file names", () => {
    expect(getFileTypeIconHref("src/App.TSX", false)).toBe(
      "/file-type-icons/sprite.svg#react_ts",
    );
    expect(getFileTypeIconHref("data.csv", false)).toBe(
      "/file-type-icons/sprite.svg#table",
    );
  });

  it("falls back for unknown, extensionless, and empty file names", () => {
    expect(getFileTypeIconHref("archive.unknownext", false)).toBe(
      "/file-type-icons/sprite.svg#document",
    );
    expect(getFileTypeIconHref("README", false)).toBe(
      "/file-type-icons/sprite.svg#readme",
    );
    expect(getFileTypeIconHref("", false)).toBe(
      "/file-type-icons/sprite.svg#document",
    );
  });

  it("uses a valid extension itself when no explicit extension mapping exists", () => {
    expect(getFileTypeIconHref("crate.rust", false)).toBe(
      "/file-type-icons/sprite.svg#rust",
    );
  });

  it("resolves directory icons and expanded open-state variants", () => {
    expect(getFileTypeIconHref("src", true)).toBe(
      "/file-type-icons/sprite.svg#folder-src",
    );
    expect(getFileTypeIconHref("src", true, true)).toBe(
      "/file-type-icons/sprite.svg#folder-src-open",
    );
    expect(getFileTypeIconHref("unknown-folder", true, true)).toBe(
      "/file-type-icons/sprite.svg#folder-open",
    );
  });
});
