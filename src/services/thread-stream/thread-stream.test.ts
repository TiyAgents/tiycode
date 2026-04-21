import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  threadCancelRunMock,
} = vi.hoisted(() => ({
  threadCancelRunMock: vi.fn<(...args: unknown[]) => Promise<boolean>>(),
}));

vi.mock("@/services/bridge", () => ({
  threadCancelRun: threadCancelRunMock,
  threadExecuteApprovedPlan: vi.fn(),
  threadStartRun: vi.fn(),
  threadSubscribeRun: vi.fn(),
  toolApprovalRespond: vi.fn(),
  toolClarifyRespond: vi.fn(),
}));

import { ThreadStream } from "@/services/thread-stream/thread-stream";

describe("ThreadStream.cancelRun", () => {
  beforeEach(() => {
    threadCancelRunMock.mockReset();
  });

  it("透传后端的幂等取消结果，不触发错误回调", async () => {
    const stream = new ThreadStream();
    const onError = vi.fn();
    stream.onError = onError;

    threadCancelRunMock.mockResolvedValueOnce(false);

    await expect(stream.cancelRun("thread-1")).resolves.toBe(false);
    expect(threadCancelRunMock).toHaveBeenCalledWith("thread-1");
    expect(onError).not.toHaveBeenCalled();
  });

  it("在真实取消失败时仍然上报错误并抛出异常", async () => {
    const stream = new ThreadStream();
    const onError = vi.fn();
    stream.onError = onError;

    threadCancelRunMock.mockRejectedValueOnce(new Error("cancel failed"));

    await expect(stream.cancelRun("thread-2")).rejects.toThrow("cancel failed");
    expect(onError).toHaveBeenCalledWith("cancel failed", "");
  });
});
