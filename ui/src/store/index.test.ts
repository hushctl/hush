import { describe, it, expect, beforeEach } from "vitest";
import { useStore, nsKey, splitKey } from "./index";

describe("nsKey", () => {
  it("combines machineId and rawId with colon separator", () => {
    expect(nsKey("machine-1", "wt-abc")).toBe("machine-1:wt-abc");
  });

  it("handles empty strings", () => {
    expect(nsKey("", "id")).toBe(":id");
    expect(nsKey("m", "")).toBe("m:");
  });

  it("handles IDs that already contain colons", () => {
    expect(nsKey("m1", "a:b")).toBe("m1:a:b");
  });
});

describe("splitKey", () => {
  it("splits a namespaced key back into machineId and rawId", () => {
    expect(splitKey("machine-1:wt-abc")).toEqual(["machine-1", "wt-abc"]);
  });

  it("returns empty machineId for keys without colon", () => {
    expect(splitKey("no-colon")).toEqual(["", "no-colon"]);
  });

  it("splits on first colon only (rawId may contain colons)", () => {
    expect(splitKey("m1:a:b")).toEqual(["m1", "a:b"]);
  });

  it("roundtrips with nsKey", () => {
    const [m, r] = splitKey(nsKey("machine-x", "raw-id-123"));
    expect(m).toBe("machine-x");
    expect(r).toBe("raw-id-123");
  });
});

describe("store", () => {
  beforeEach(() => {
    useStore.setState(useStore.getInitialState());
  });

  describe("handleServerMessage", () => {
    it("ignores malformed JSON", () => {
      const before = useStore.getState();
      useStore.getState().handleServerMessage("not json");
      const after = useStore.getState();
      // projects/worktrees should be unchanged
      expect(after.projects).toEqual(before.projects);
      expect(after.worktrees).toEqual(before.worktrees);
    });

    it("processes project_list and namespaces project IDs", () => {
      const msg = JSON.stringify({
        type: "project_list",
        machine_id: "m1",
        projects: [
          {
            id: "p1",
            name: "my-project",
            path: "/tmp/proj",
            worktree_count: 2,
            machine_id: "m1",
          },
        ],
      });
      useStore.getState().handleServerMessage(msg);
      const state = useStore.getState();
      expect(state.projects["m1:p1"]).toBeDefined();
      expect(state.projects["m1:p1"].name).toBe("my-project");
      expect(state.projects["m1:p1"].id).toBe("m1:p1");
      expect(state.projects["m1:p1"].machine_id).toBe("m1");
    });

    it("project_list replaces stale entries for same machine", () => {
      // First list
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "project_list",
          machine_id: "m1",
          projects: [
            { id: "old", name: "old-proj", path: "/tmp/old", worktree_count: 1, machine_id: "m1" },
          ],
        }),
      );
      expect(useStore.getState().projects["m1:old"]).toBeDefined();

      // Second list replaces it
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "project_list",
          machine_id: "m1",
          projects: [
            { id: "new", name: "new-proj", path: "/tmp/new", worktree_count: 1, machine_id: "m1" },
          ],
        }),
      );
      const state = useStore.getState();
      expect(state.projects["m1:old"]).toBeUndefined();
      expect(state.projects["m1:new"]).toBeDefined();
    });

    it("processes worktree_list and namespaces both worktree and project IDs", () => {
      const msg = JSON.stringify({
        type: "worktree_list",
        machine_id: "m1",
        worktrees: [
          {
            id: "wt-1",
            project_id: "p1",
            branch: "main",
            working_dir: "/tmp/wt",
            status: "idle",
            last_task: null,
            session_id: null,
            machine_id: "m1",
          },
        ],
      });
      useStore.getState().handleServerMessage(msg);
      const state = useStore.getState();
      const wt = state.worktrees["m1:wt-1"];
      expect(wt).toBeDefined();
      expect(wt.id).toBe("m1:wt-1");
      expect(wt.project_id).toBe("m1:p1");
      expect(wt.machine_id).toBe("m1");
      expect(wt.branch).toBe("main");
    });

    it("processes status_change for existing worktree", () => {
      // Seed a worktree
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "worktree_list",
          machine_id: "m1",
          worktrees: [
            {
              id: "wt-1",
              project_id: "p1",
              branch: "main",
              working_dir: "/tmp",
              status: "idle",
              last_task: null,
              session_id: null,
              machine_id: "m1",
            },
          ],
        }),
      );
      expect(useStore.getState().worktrees["m1:wt-1"].status).toBe("idle");

      // Update status
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "status_change",
          machine_id: "m1",
          worktree_id: "wt-1",
          status: "running",
        }),
      );
      expect(useStore.getState().worktrees["m1:wt-1"].status).toBe("running");
    });

    it("status_change is a no-op for unknown worktree", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "status_change",
          machine_id: "m1",
          worktree_id: "nonexistent",
          status: "running",
        }),
      );
      expect(useStore.getState().worktrees["m1:nonexistent"]).toBeUndefined();
    });

    it("processes error messages and sets daemonError", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "error",
          machine_id: "m1",
          message: "something broke",
          worktree_id: null,
        }),
      );
      expect(useStore.getState().daemonError).toBe("something broke");
    });

    it("processes path_not_found and sets pendingCreate", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "path_not_found",
          machine_id: "m1",
          path: "/nonexistent/path",
          name: "my-project",
        }),
      );
      const state = useStore.getState();
      expect(state.pendingCreate).toEqual({
        path: "/nonexistent/path",
        name: "my-project",
        machineId: "m1",
      });
    });

    it("processes git_status and stores it by namespaced key", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "git_status",
          machine_id: "m1",
          worktree_id: "wt-1",
          staged: ["file1.ts"],
          modified: ["file2.ts"],
          untracked: ["file3.ts"],
        }),
      );
      const gs = useStore.getState().gitStatus["m1:wt-1"];
      expect(gs).toEqual({
        staged: ["file1.ts"],
        modified: ["file2.ts"],
        untracked: ["file3.ts"],
      });
    });

    it("processes file_list and stores it by namespaced key", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "file_list",
          machine_id: "m1",
          worktree_id: "wt-1",
          files: ["src/index.ts", "package.json"],
        }),
      );
      expect(useStore.getState().fileList["m1:wt-1"]).toEqual([
        "src/index.ts",
        "package.json",
      ]);
    });

    it("processes file_content and stores it by namespaced key", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "file_content",
          machine_id: "m1",
          worktree_id: "wt-1",
          path: "src/index.ts",
          content: "console.log('hello');",
          truncated: false,
        }),
      );
      const fc = useStore.getState().fileContents["m1:wt-1"];
      expect(fc).toEqual({
        path: "src/index.ts",
        content: "console.log('hello');",
        truncated: false,
      });
    });

    it("processes memory_pressure and stores alerts", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "memory_pressure",
          machine_id: "m1",
          level: "warning",
          available_bytes: 500_000_000,
          total_bytes: 16_000_000_000,
        }),
      );
      const alert = useStore.getState().memoryAlerts["m1"];
      expect(alert).toBeDefined();
      expect(alert.level).toBe("warning");
      expect(alert.availableBytes).toBe(500_000_000);
      expect(alert.totalBytes).toBe(16_000_000_000);
    });

    it("memory_pressure normal level clears the alert", () => {
      // Set a warning first
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "memory_pressure",
          machine_id: "m1",
          level: "warning",
          available_bytes: 500_000_000,
          total_bytes: 16_000_000_000,
        }),
      );
      expect(useStore.getState().memoryAlerts["m1"]).toBeDefined();

      // Clear with normal
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "memory_pressure",
          machine_id: "m1",
          level: "normal",
          available_bytes: 8_000_000_000,
          total_bytes: 16_000_000_000,
        }),
      );
      expect(useStore.getState().memoryAlerts["m1"]).toBeUndefined();
    });

    it("memory_pressure appends samples for sparkline", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "memory_pressure",
          machine_id: "m1",
          level: "warning",
          available_bytes: 4_000_000_000,
          total_bytes: 16_000_000_000,
        }),
      );
      const samples = useStore.getState().memorySamples["m1"];
      expect(samples).toHaveLength(1);
      expect(samples[0].ratio).toBeCloseTo(0.25);
    });

    it("transfer_progress creates a new transfer entry", () => {
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "transfer_progress",
          machine_id: "m1",
          transfer_id: "tx-1",
          phase: "streaming",
          bytes_sent: 5000,
          total_bytes: 10000,
          source_worktree_id: "wt-1",
          project_name: "proj",
          branch: "main",
          dest_machine_id: "m2",
        }),
      );
      const transfer = useStore.getState().transfers["tx-1"];
      expect(transfer).toBeDefined();
      expect(transfer.phase).toBe("streaming");
      expect(transfer.bytesSent).toBe(5000);
      expect(transfer.totalBytes).toBe(10000);
      expect(transfer.projectName).toBe("proj");
      expect(transfer.branch).toBe("main");
    });

    it("transfer_error marks transfer as failed with error message", () => {
      // Seed a transfer
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "transfer_progress",
          machine_id: "m1",
          transfer_id: "tx-1",
          phase: "streaming",
          bytes_sent: 5000,
          total_bytes: 10000,
          source_worktree_id: "wt-1",
          project_name: "proj",
          branch: "main",
          dest_machine_id: "m2",
        }),
      );

      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "transfer_error",
          machine_id: "m1",
          transfer_id: "tx-1",
          message: "connection lost",
        }),
      );
      const transfer = useStore.getState().transfers["tx-1"];
      expect(transfer.phase).toBe("failed");
      expect(transfer.errorMessage).toBe("connection lost");
    });

    it("transfer_complete marks transfer as complete", () => {
      // Seed a transfer
      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "transfer_progress",
          machine_id: "m1",
          transfer_id: "tx-1",
          phase: "streaming",
          bytes_sent: 10000,
          total_bytes: 10000,
          source_worktree_id: "wt-1",
          project_name: "proj",
          branch: "main",
          dest_machine_id: "m2",
        }),
      );

      useStore.getState().handleServerMessage(
        JSON.stringify({
          type: "transfer_complete",
          machine_id: "m1",
          transfer_id: "tx-1",
          new_worktree_id: "wt-2",
        }),
      );
      expect(useStore.getState().transfers["tx-1"].phase).toBe("complete");
    });
  });

  describe("simple actions", () => {
    it("clearDaemonError resets daemonError to null", () => {
      useStore.setState({ daemonError: "some error" });
      useStore.getState().clearDaemonError();
      expect(useStore.getState().daemonError).toBeNull();
    });

    it("clearPendingCreate resets pendingCreate to null", () => {
      useStore.setState({
        pendingCreate: { path: "/tmp", name: "test", machineId: "m1" },
      });
      useStore.getState().clearPendingCreate();
      expect(useStore.getState().pendingCreate).toBeNull();
    });

    it("selectWorktree sets selectedWorktreeId", () => {
      useStore.getState().selectWorktree("m1:wt-1");
      expect(useStore.getState().selectedWorktreeId).toBe("m1:wt-1");

      useStore.getState().selectWorktree(null);
      expect(useStore.getState().selectedWorktreeId).toBeNull();
    });

    it("switchToGrid sets layoutMode to grid", () => {
      useStore.setState({ layoutMode: "canvas" });
      useStore.getState().switchToGrid();
      expect(useStore.getState().layoutMode).toBe("grid");
    });

    it("switchToCanvas sets layoutMode to canvas", () => {
      useStore.getState().switchToCanvas();
      expect(useStore.getState().layoutMode).toBe("canvas");
    });

    it("setTileMode updates tileMode", () => {
      useStore.getState().setTileMode("2-up");
      expect(useStore.getState().tileMode).toBe("2-up");
    });
  });

  describe("daemon management", () => {
    it("setDaemonConnected updates connection status", () => {
      useStore.getState().setDaemonConnected("localhost", true);
      expect(useStore.getState().daemons["localhost"].connected).toBe(true);

      useStore.getState().setDaemonConnected("localhost", false);
      expect(useStore.getState().daemons["localhost"].connected).toBe(false);
    });

    it("setDaemonConnected is a no-op for unknown machine", () => {
      useStore.getState().setDaemonConnected("unknown", true);
      expect(useStore.getState().daemons["unknown"]).toBeUndefined();
    });

    it("addDaemon registers a new daemon entry", () => {
      useStore.getState().addDaemon({
        id: "remote-1",
        name: "Remote Machine",
        url: "wss://192.168.1.5:9111/ws",
      });
      const daemon = useStore.getState().daemons["remote-1"];
      expect(daemon).toBeDefined();
      expect(daemon.name).toBe("Remote Machine");
      expect(daemon.connected).toBe(false);
    });

    it("removeDaemon removes daemon and its projects/worktrees", () => {
      // Add daemon with projects and worktrees
      useStore.getState().addDaemon({
        id: "m2",
        name: "Machine 2",
        url: "wss://m2:9111/ws",
      });
      useStore.setState((s) => ({
        projects: {
          ...s.projects,
          "m2:p1": {
            id: "m2:p1",
            name: "proj",
            path: "/tmp",
            worktree_count: 1,
            machine_id: "m2",
          },
        },
        worktrees: {
          ...s.worktrees,
          "m2:wt-1": {
            id: "m2:wt-1",
            project_id: "m2:p1",
            branch: "main",
            working_dir: "/tmp",
            status: "idle" as const,
            last_task: null,
            session_id: null,
            machine_id: "m2",
          },
        },
      }));

      useStore.getState().removeDaemon("m2");
      const state = useStore.getState();
      expect(state.daemons["m2"]).toBeUndefined();
      expect(state.projects["m2:p1"]).toBeUndefined();
      expect(state.worktrees["m2:wt-1"]).toBeUndefined();
    });

    it("mergeDiscoveredPeers adds unknown peers as daemon entries", () => {
      const peers = [
        { machine_id: "peer-1", url: "wss://peer1:9111/ws", last_seen: 1000 },
        { machine_id: "peer-2", url: "wss://peer2:9111/ws", last_seen: 2000 },
      ];
      useStore.getState().mergeDiscoveredPeers(peers);
      const daemons = useStore.getState().daemons;
      expect(daemons["peer-1"]).toBeDefined();
      expect(daemons["peer-2"]).toBeDefined();
      expect(daemons["peer-1"].url).toBe("wss://peer1:9111/ws");
    });

    it("mergeDiscoveredPeers skips already-known machines", () => {
      useStore.getState().addDaemon({
        id: "known",
        name: "Known Machine",
        url: "wss://known:9111/ws",
      });
      const before = useStore.getState().daemons["known"];

      useStore.getState().mergeDiscoveredPeers([
        { machine_id: "known", url: "wss://different-url:9111/ws", last_seen: 1000 },
      ]);
      // Should not overwrite existing entry
      expect(useStore.getState().daemons["known"].url).toBe(before.url);
    });

    it("resolveDaemonId renames temp entry to real machine_id", () => {
      // The initial localhost entry
      const before = useStore.getState().daemons["localhost"];
      expect(before).toBeDefined();

      useStore.getState().resolveDaemonId("localhost", "real-machine-id");
      const daemons = useStore.getState().daemons;
      expect(daemons["localhost"]).toBeUndefined();
      expect(daemons["real-machine-id"]).toBeDefined();
      expect(daemons["real-machine-id"].id).toBe("real-machine-id");
    });

    it("resolveDaemonId is a no-op when tempId equals realId", () => {
      const before = { ...useStore.getState().daemons };
      useStore.getState().resolveDaemonId("localhost", "localhost");
      expect(useStore.getState().daemons).toEqual(before);
    });
  });

  describe("dismissTransfer", () => {
    it("removes the specified transfer", () => {
      useStore.setState({
        transfers: {
          "tx-1": {
            transferId: "tx-1",
            phase: "failed",
            bytesSent: 0,
            totalBytes: 0,
            sourceMachineId: "m1",
            destMachineId: "m2",
            sourceWorktreeKey: "m1:wt-1",
            projectName: "proj",
            branch: "main",
            errorMessage: "oops",
          },
        },
      });
      useStore.getState().dismissTransfer("tx-1");
      expect(useStore.getState().transfers["tx-1"]).toBeUndefined();
    });
  });

  describe("file viewer actions", () => {
    it("openFileContent stores file content for a worktree", () => {
      useStore.getState().openFileContent("m1:wt-1", "src/app.ts", "const x = 1;", false);
      const fc = useStore.getState().fileContents["m1:wt-1"];
      expect(fc).toEqual({
        path: "src/app.ts",
        content: "const x = 1;",
        truncated: false,
      });
    });

    it("clearFileContent removes file content for a worktree", () => {
      useStore.getState().openFileContent("m1:wt-1", "src/app.ts", "code", false);
      useStore.getState().clearFileContent("m1:wt-1");
      expect(useStore.getState().fileContents["m1:wt-1"]).toBeUndefined();
    });
  });

  describe("cmd+P modal", () => {
    it("openCmdP sets modal state", () => {
      useStore.getState().openCmdP("m1:wt-1");
      const state = useStore.getState();
      expect(state.cmdPOpen).toBe(true);
      expect(state.cmdPTargetWorktree).toBe("m1:wt-1");
    });

    it("closeCmdP clears modal state", () => {
      useStore.getState().openCmdP("m1:wt-1");
      useStore.getState().closeCmdP();
      const state = useStore.getState();
      expect(state.cmdPOpen).toBe(false);
      expect(state.cmdPTargetWorktree).toBeNull();
    });
  });
});
