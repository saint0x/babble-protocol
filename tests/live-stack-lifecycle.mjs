import { spawn } from "node:child_process";
import { rm } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";

/** Own only processes started for one disposable live-stack store. */
export class LiveStackLifecycle {
  #children = new Set();
  #owned = new WeakMap();
  #cleanup;
  #stopping = false;
  #removeSignals;
  #signal;

  constructor(storeRoot, { graceMs = 10_000, killMs = 2_000, beforeStop = null } = {}) {
    if (process.platform === "win32") throw new Error("Live-stack process groups require a POSIX host");
    if (!storeRoot || !Number.isFinite(graceMs) || graceMs <= 0 || !Number.isFinite(killMs) || killMs <= 0
      || (beforeStop !== null && typeof beforeStop !== "function")) {
      throw new Error("Expected a disposable store and bounded shutdown durations");
    }
    this.storeRoot = storeRoot;
    this.graceMs = graceMs;
    this.killMs = killMs;
    this.beforeStop = beforeStop;
  }

  get stopping() { return this.#stopping; }

  spawn(command, args, options) {
    if (this.#stopping) throw new Error("Live-stack is stopping; cannot start another process");
    // A dedicated group includes Cargo/API/Python descendants, without touching
    // the shell's group or an independently running preview on another port.
    const child = spawn(command, args, { ...options, detached: true });
    const record = { child, pid: child.pid, stopped: null };
    record.finished = new Promise(resolve => {
      child.once("exit", resolve);
      child.once("error", resolve);
    });
    this.#children.add(record);
    this.#owned.set(child, record);
    return child;
  }

  stop(child) {
    if (!child) return Promise.resolve();
    const record = this.#owned.get(child);
    if (!record) return Promise.reject(new Error("Refusing to stop a process not owned by this live stack"));
    if (!record.stopped) record.stopped = this.#stopGroup(record).then(() => { this.#children.delete(record); });
    return record.stopped;
  }

  async #stopGroup(record) {
    if (!record.pid) { await record.finished; return; }
    const signal = name => {
      try { process.kill(-record.pid, name); return true; }
      catch (error) {
        if (error.code === "ESRCH") return false;
        // macOS can report EPERM while an exited group is being reaped. A
        // failed existence probe still means wait, never that cleanup passed.
        if (name === 0 && error.code === "EPERM") return true;
        throw error;
      }
    };
    const gone = async timeoutMs => {
      const deadline = performance.now() + timeoutMs;
      while (signal(0)) {
        if (performance.now() >= deadline) return false;
        await delay(Math.min(25, Math.max(1, deadline - performance.now())));
      }
      return true;
    };
    if (signal("SIGINT") && !await gone(this.graceMs)) {
      signal("SIGKILL");
      if (!await gone(this.killMs)) throw new Error(`Owned process group ${record.pid} did not stop after SIGKILL`);
    }
    await record.finished;
  }

  cleanup() {
    if (this.#cleanup) return this.#cleanup;
    // Close admission synchronously before any await, including API restarts.
    this.#stopping = true;
    this.#cleanup = (async () => {
      const errors = [];
      if (this.beforeStop) {
        let timer;
        try {
          await Promise.race([
            Promise.resolve().then(() => this.beforeStop()),
            new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Live-stack beforeStop cleanup timed out")), this.graceMs); }),
          ]);
        } catch (error) { errors.push(error); }
        finally { clearTimeout(timer); }
      }
      const outcomes = await Promise.allSettled([...this.#children].map(record => this.stop(record.child)));
      errors.push(...outcomes.filter(outcome => outcome.status === "rejected").map(outcome => outcome.reason));
      if (errors.length) throw new AggregateError(errors, "Live-stack process cleanup failed; disposable store retained");
      await rm(this.storeRoot, { recursive: true, force: true });
    })().finally(() => { this.#removeSignals?.(); });
    return this.#cleanup;
  }

  installSignalHandlers() {
    if (this.#removeSignals) throw new Error("Live-stack signal handlers are already installed");
    const stop = signal => {
      if (this.#signal) return;
      this.#signal = signal;
      // The running acceptance may be awaiting an unrelated browser response.
      // Exit only after owned process groups have stopped and the store is gone.
      void this.cleanup().then(() => process.exit(signal === "SIGINT" ? 130 : 143), error => {
        console.error(error);
        process.exit(1);
      });
    };
    const interrupt = () => stop("SIGINT"), terminate = () => stop("SIGTERM");
    const outputError = error => {
      if (error.code === "EPIPE") terminate();
      else throw error;
    };
    process.on("SIGINT", interrupt);
    process.on("SIGTERM", terminate);
    if (process.send) process.on("disconnect", terminate);
    // A killed evidence wrapper also closes its log pipes. Forwarded child
    // output must not crash this owner halfway through descendant cleanup.
    process.stdout.on("error", outputError);
    process.stderr.on("error", outputError);
    this.#removeSignals = () => {
      process.off("SIGINT", interrupt);
      process.off("SIGTERM", terminate);
      process.off("disconnect", terminate);
      process.stdout.off("error", outputError);
      process.stderr.off("error", outputError);
    };
    if (process.send && !process.connected) terminate();
  }
}
