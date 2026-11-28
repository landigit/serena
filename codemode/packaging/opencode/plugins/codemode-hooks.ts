import { execFileSync } from "node:child_process";

function runHook(event: string, payload: unknown): Record<string, unknown> {
  try {
    const out = execFileSync("codemode", ["hook", event], {
      input: JSON.stringify(payload ?? {}),
      encoding: "utf8",
      timeout: 5000,
    });
    return JSON.parse(out || "{}") as Record<string, unknown>;
  } catch {
    return { allow: true };
  }
}

export default async function codemodeHooksPlugin() {
  return {
    "tool.execute.before": async (input: { tool: string; args?: Record<string, unknown> }) => {
      const decision = runHook("pre-tool", {
        tool_name: input.tool,
        command: input.args?.command,
        file_path: input.args?.filePath ?? input.args?.path,
      });
      if (decision.allow === false) {
        throw new Error(String(decision.reason ?? "blocked by codemode policy"));
      }
    },
    "tool.execute.after": async (
      input: { tool: string },
      output: { output?: string },
    ) => {
      if (typeof output?.output !== "string") return;
      const res = runHook("post-tool", {
        tool_name: input.tool,
        output: output.output,
      });
      if (typeof res.throttled_output === "string") {
        output.output = res.throttled_output;
      }
    },
    "experimental.session.compacting": async (
      _input: unknown,
      output: { context?: string[] },
    ) => {
      const res = runHook("pre-compact", {});
      if (typeof res.additional_context === "string" && Array.isArray(output?.context)) {
        output.context.push(res.additional_context);
      }
    },
  };
}
