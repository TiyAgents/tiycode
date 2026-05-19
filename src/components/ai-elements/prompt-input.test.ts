import { describe, expect, it } from "vitest";

import {
  createAttachmentFileBatchKey,
  dedupeAttachmentFileBatch,
  dedupePastedAttachmentFileBatch,
  shouldIgnoreDuplicatePasteBatch,
} from "./prompt-input";

const createFile = ({
  lastModified = 1,
  name,
  size,
  type,
}: {
  lastModified?: number;
  name: string;
  size: number;
  type: string;
}) =>
  new File([new Uint8Array(size)], name, {
    lastModified,
    type,
  });

describe("prompt input attachment paste dedupe", () => {
  it("keeps only the first pasted image flavor from one clipboard batch", () => {
    const png = createFile({ name: "image.png", size: 12, type: "image/png" });
    const tiff = createFile({ name: "image.tiff", size: 24, type: "image/tiff" });
    const text = createFile({ name: "notes.txt", size: 8, type: "text/plain" });

    expect(dedupePastedAttachmentFileBatch([png, tiff, text])).toEqual([png, text]);
  });

  it("treats empty-type generic clipboard image entries as duplicate image flavors", () => {
    const emptyType = createFile({ name: "image", size: 10, type: "" });
    const png = createFile({ name: "image.png", size: 12, type: "image/png" });

    expect(dedupePastedAttachmentFileBatch([emptyType, png])).toEqual([
      emptyType,
    ]);
    expect(dedupePastedAttachmentFileBatch([png, emptyType])).toEqual([png]);
  });

  it("keeps distinct pasted image files that do not look like alternate clipboard flavors", () => {
    const first = createFile({ name: "first.png", size: 12, type: "image/png" });
    const second = createFile({ name: "second.png", size: 16, type: "image/png" });

    expect(dedupePastedAttachmentFileBatch([first, second])).toEqual([
      first,
      second,
    ]);
  });

  it("deduplicates repeated files in generic attachment batches without dropping distinct images", () => {
    const first = createFile({ name: "a.png", size: 12, type: "image/png" });
    const duplicate = createFile({ name: "a.png", size: 12, type: "image/png" });
    const second = createFile({ name: "b.png", size: 16, type: "image/png" });

    expect(dedupeAttachmentFileBatch([first, duplicate, second])).toEqual([
      first,
      second,
    ]);
  });

  it("detects duplicate paste batches only inside the short guard window", () => {
    const file = createFile({ name: "image.png", size: 12, type: "image/png" });
    const batchKey = createAttachmentFileBatchKey([file]);
    const lastBatch = { key: batchKey, timestamp: 1_000 };

    expect(
      shouldIgnoreDuplicatePasteBatch({ batchKey, lastBatch, now: 1_500 })
    ).toBe(true);
    expect(
      shouldIgnoreDuplicatePasteBatch({ batchKey, lastBatch, now: 2_000 })
    ).toBe(false);
    expect(
      shouldIgnoreDuplicatePasteBatch({
        batchKey: `${batchKey}-next`,
        lastBatch,
        now: 1_500,
      })
    ).toBe(false);
  });
});
