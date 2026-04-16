import { describe, it, expect } from "vitest";
import type {
  ProjectInfo,
  WorktreeInfo,
  WorktreeStatus,
  PeerInfo,
  ClientMessage,
  ServerMessage,
} from "./protocol";

describe("protocol types", () => {
  it("ProjectInfo satisfies the expected shape", () => {
    const project: ProjectInfo = {
      id: "proj-1",
      name: "my-project",
      path: "/home/user/projects/my-project",
      worktree_count: 3,
      machine_id: "machine-abc",
    };
    expect(project.id).toBe("proj-1");
    expect(project.name).toBe("my-project");
    expect(project.worktree_count).toBe(3);
  });

  it("WorktreeInfo supports all status variants", () => {
    const base = {
      id: "wt-1",
      project_id: "proj-1",
      branch: "main",
      working_dir: "/tmp/wt",
      last_task: null,
      session_id: null,
      machine_id: "m1",
    };

    const idle: WorktreeInfo = { ...base, status: "idle" };
    const running: WorktreeInfo = { ...base, status: "running" };
    const needsYou: WorktreeInfo = { ...base, status: "needs_you" };
    const failed: WorktreeInfo = { ...base, status: "failed: oom killed" };

    expect(idle.status).toBe("idle");
    expect(running.status).toBe("running");
    expect(needsYou.status).toBe("needs_you");
    expect(failed.status).toBe("failed: oom killed");
  });

  it("WorktreeStatus template literal accepts arbitrary failure reasons", () => {
    const status: WorktreeStatus = "failed: connection timeout";
    expect(status).toContain("failed:");
  });

  it("WorktreeInfo shell_alive is optional", () => {
    const wt: WorktreeInfo = {
      id: "wt-2",
      project_id: "proj-1",
      branch: "feature",
      working_dir: "/tmp/wt2",
      status: "idle",
      last_task: "do something",
      session_id: "sess-1",
      machine_id: "m1",
    };
    expect(wt.shell_alive).toBeUndefined();

    const wtWithShell: WorktreeInfo = { ...wt, shell_alive: true };
    expect(wtWithShell.shell_alive).toBe(true);
  });

  it("PeerInfo has the expected fields", () => {
    const peer: PeerInfo = {
      machine_id: "peer-1",
      url: "wss://192.168.1.5:9111/ws",
      last_seen: 1700000000,
    };
    expect(peer.url).toContain("wss://");
    expect(peer.last_seen).toBeGreaterThan(0);
  });

  describe("ClientMessage discriminated union", () => {
    it("register_project message", () => {
      const msg: ClientMessage = {
        type: "register_project",
        path: "/home/user/project",
        name: "project",
      };
      expect(msg.type).toBe("register_project");
    });

    it("pty_attach message includes cols and rows", () => {
      const msg: ClientMessage = {
        type: "pty_attach",
        worktree_id: "wt-1",
        cols: 80,
        rows: 24,
      };
      expect(msg.type).toBe("pty_attach");
      expect(msg.cols).toBe(80);
      expect(msg.rows).toBe(24);
    });

    it("create_worktree allows optional permission_mode", () => {
      const msg: ClientMessage = {
        type: "create_worktree",
        project_id: "proj-1",
        branch: "feature-x",
      };
      expect(msg.type).toBe("create_worktree");

      const msgWithPerm: ClientMessage = {
        type: "create_worktree",
        project_id: "proj-1",
        branch: "feature-x",
        permission_mode: "plan",
      };
      expect(msgWithPerm.permission_mode).toBe("plan");
    });

    it("paste_image message includes data and optional filename", () => {
      const msg: ClientMessage = {
        type: "paste_image",
        worktree_id: "wt-1",
        data: "base64data",
      };
      expect(msg.type).toBe("paste_image");

      const msgWithFile: ClientMessage = {
        type: "paste_image",
        worktree_id: "wt-1",
        data: "base64data",
        filename: "screenshot.png",
      };
      expect(msgWithFile.filename).toBe("screenshot.png");
    });

    it("transfer_worktree message", () => {
      const msg: ClientMessage = {
        type: "transfer_worktree",
        worktree_id: "wt-1",
        dest_machine_id: "machine-2",
      };
      expect(msg.type).toBe("transfer_worktree");
    });
  });

  describe("ServerMessage discriminated union", () => {
    it("status_change message", () => {
      const msg: ServerMessage = {
        type: "status_change",
        machine_id: "m1",
        worktree_id: "wt-1",
        status: "running",
      };
      expect(msg.type).toBe("status_change");
    });

    it("error message with nullable worktree_id", () => {
      const msg: ServerMessage = {
        type: "error",
        machine_id: "m1",
        message: "something went wrong",
        worktree_id: null,
      };
      expect(msg.type).toBe("error");
      expect(msg.worktree_id).toBeNull();
    });

    it("memory_pressure message with levels", () => {
      const msg: ServerMessage = {
        type: "memory_pressure",
        machine_id: "m1",
        level: "critical",
        available_bytes: 100_000_000,
        total_bytes: 16_000_000_000,
      };
      expect(msg.type).toBe("memory_pressure");
      expect(msg.level).toBe("critical");
    });

    it("transfer_progress message includes all phases", () => {
      const msg: ServerMessage = {
        type: "transfer_progress",
        machine_id: "m1",
        transfer_id: "tx-1",
        phase: "streaming",
        bytes_sent: 5000,
        total_bytes: 10000,
        source_worktree_id: "wt-1",
        project_name: "my-project",
        branch: "main",
        dest_machine_id: "m2",
      };
      expect(msg.type).toBe("transfer_progress");
      expect(msg.phase).toBe("streaming");
    });

    it("project_list and worktree_list carry arrays", () => {
      const projMsg: ServerMessage = {
        type: "project_list",
        machine_id: "m1",
        projects: [
          {
            id: "p1",
            name: "proj",
            path: "/tmp",
            worktree_count: 1,
            machine_id: "m1",
          },
        ],
      };
      expect(projMsg.type).toBe("project_list");

      const wtMsg: ServerMessage = {
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
      };
      expect(wtMsg.type).toBe("worktree_list");
    });
  });
});
