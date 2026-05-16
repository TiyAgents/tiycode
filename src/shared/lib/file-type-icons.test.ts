import { describe, expect, it } from "vitest";
import { FILE_TYPE_ICON_IDS } from "@/shared/lib/file-type-icon-ids";
import {
  EXTENSION_ICON_ID_MAP,
  FILE_NAME_ICON_ID_MAP,
  FOLDER_NAME_ICON_ID_MAP,
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
