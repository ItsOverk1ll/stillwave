// SPDX-License-Identifier: GPL-3.0-only
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as api from "./api";

vi.mock("./api");

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("App", () => {
  it("shows versions, schema and database path from the Rust side", async () => {
    vi.mocked(api.getAppInfo).mockResolvedValue({
      appName: "Stillwave",
      appVersion: "0.0.1",
      coreVersion: "0.0.1",
      schemaVersion: 1,
      dbPath: "C:\\data\\stillwave.db",
    });
    render(<App />);
    expect(await screen.findByText("v1")).toBeTruthy();
    expect(screen.getByText("C:\\data\\stillwave.db")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Stillwave" })).toBeTruthy();
  });

  it("explains itself when the Rust core is unreachable", async () => {
    vi.mocked(api.getAppInfo).mockRejectedValue(new Error("no tauri"));
    render(<App />);
    expect((await screen.findByRole("alert")).textContent).toContain("no tauri");
  });
});
