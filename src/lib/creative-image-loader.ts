export const CREATIVE_IMAGE_RETRY_MS = 2_000;
export const CREATIVE_IMAGE_WAIT_MS = 60_000;

export type CreativeImageState =
  | { status: "loading" | "pending" }
  | { status: "ready"; url: string }
  | { status: "error"; error: string };

type Entry = {
  runId: string;
  idx: number;
  state: CreativeImageState;
  inFlight: boolean;
  generation: number;
  retryRequested: boolean;
  waitForRun: boolean;
  retryTimer?: ReturnType<typeof setTimeout>;
  deadlineTimer?: ReturnType<typeof setTimeout>;
};

export const creativeImageKey = (runId: string, idx: number) =>
  `${runId}:${idx}`;

/** Preview retrieval has its own lifetime: scoring/run polling may finish first. */
export function createCreativeImageLoader(
  fetchImage: (runId: string, idx: number) => Promise<string>,
  onState: (key: string, state: CreativeImageState) => void,
  describeError: (error: unknown) => string,
) {
  const entries = new Map<string, Entry>();
  let disposed = false;

  function publish(entry: Entry, state: CreativeImageState) {
    entry.state = state;
    if (!disposed) onState(creativeImageKey(entry.runId, entry.idx), state);
  }
  function clearTimers(entry: Entry) {
    clearTimeout(entry.retryTimer);
    clearTimeout(entry.deadlineTimer);
    entry.retryTimer = entry.deadlineTimer = undefined;
  }
  function fail(entry: Entry, error: string) {
    clearTimers(entry);
    ++entry.generation;
    publish(entry, { status: "error", error });
  }
  function startDeadline(entry: Entry) {
    // Long-running generation/scoring is normal. Bound only the publication
    // delay once the run ends, or when browsing unmonitored history.
    if (!entry.waitForRun && entry.deadlineTimer === undefined) {
      entry.deadlineTimer = setTimeout(() => {
        fail(entry, "图片仍未就绪，自动取图已等待 60 秒。可以重试取图。");
      }, CREATIVE_IMAGE_WAIT_MS);
    }
  }
  async function fetch(entry: Entry) {
    if (disposed || entry.inFlight) return;
    const generation = entry.generation;
    entry.inFlight = true;
    try {
      const url = await fetchImage(entry.runId, entry.idx);
      if (disposed || entry.generation !== generation) return;
      if (!url) throw new Error("图片地址为空，请重试取图。");
      clearTimers(entry);
      publish(entry, { status: "ready", url });
    } catch (error) {
      if (disposed || entry.generation !== generation) return;
      const pending =
        typeof error === "object" &&
        error !== null &&
        "code" in error &&
        error.code === "creative_image_pending";
      if (!pending) {
        fail(entry, describeError(error));
        return;
      }
      publish(entry, { status: "pending" });
      startDeadline(entry);
      entry.retryTimer = setTimeout(() => {
        entry.retryTimer = undefined;
        void fetch(entry);
      }, CREATIVE_IMAGE_RETRY_MS);
    } finally {
      entry.inFlight = false;
      if (!disposed && entry.retryRequested) {
        entry.retryRequested = false;
        void fetch(entry);
      }
    }
  }
  function ensure(
    runId: string,
    idx: number,
    waitForRun = false,
    retry = false,
  ) {
    if (disposed) return;
    const key = creativeImageKey(runId, idx);
    let entry = entries.get(key);
    if (entry) {
      // Once a run ends or becomes unmonitored, do not revert to unlimited waiting.
      entry.waitForRun &&= waitForRun;
      if (entry.state.status === "pending") startDeadline(entry);
      // Polling must neither bypass the retry interval nor hide real errors.
      if (!retry || entry.state.status !== "error") return;
    }
    if (!entry) {
      entry = {
        runId,
        idx,
        state: { status: "loading" },
        inFlight: false,
        generation: 0,
        retryRequested: false,
        waitForRun,
      };
      entries.set(key, entry);
    }
    clearTimers(entry);
    ++entry.generation;
    publish(entry, { status: "loading" });
    // A timed-out IPC call cannot be cancelled; wait for it before manual retry.
    if (entry.inFlight) entry.retryRequested = true;
    else void fetch(entry);
  }
  function dispose() {
    disposed = true;
    for (const entry of entries.values()) clearTimers(entry);
    entries.clear();
  }
  return {
    ensure,
    retry: (runId: string, idx: number) =>
      ensure(
        runId,
        idx,
        entries.get(creativeImageKey(runId, idx))?.waitForRun ?? false,
        true,
      ),
    dispose,
  };
}
