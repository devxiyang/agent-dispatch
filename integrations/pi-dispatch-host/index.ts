import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import type {
	ExtensionAPI,
	ExtensionCommandContext,
	ExtensionContext,
} from "@mariozechner/pi-coding-agent";

const STATE_ENTRY_TYPE = "dispatch-ui-state";
const STATUS_KEY = "dispatch-host";
const WIDGET_KEY = "dispatch-host-widget";
const DEFAULT_ROOT = ".dispatch";
const DEFAULT_BACKEND = "pi";
const DEFAULT_TASK_MODE = "auto";
const DEFAULT_EXECUTION_MODE = "auto";
const MAX_EVENT_LINES = 12;

type BackendName = "codex" | "claude" | "pi" | "cursor-agent";
type TaskMode = "auto" | "direct" | "plan" | "discuss";
type ExecutionMode = "standard" | "auto" | "danger";
type TemplateKind = "generic" | "feature" | "bugfix" | "refactor" | "audit" | "research";

interface DispatchUiState {
	root: string;
	lastTaskId?: string;
	lastTaskTitle?: string;
	lastTaskStatus?: string;
	lastTaskBackend?: string;
	lastUpdatedAt?: string;
}

interface DispatchTaskRecord {
	id: string;
	title: string;
	backend: string;
	model?: string | null;
	execution_mode: string;
	status: string;
	updated_at: string;
	artifacts: {
		stdout_path?: string;
		stderr_path?: string;
	};
}

interface DispatchEventRecord {
	sequence: number;
	timestamp: string;
	kind: string;
	message: string;
}

interface DispatchExecutionSummary {
	task_id: string;
	status: string;
	exit_code: number | null;
	stdout_path: string;
	stderr_path: string;
}

interface DispatchReadySummary {
	config_path: string;
	default_target: string;
	backend_count: number;
	model_count: number;
	alias_count: number;
	installed_backends: string[];
}

interface DispatchRouteSummary {
	kind: "Warmup" | "ConfigRequest" | "TaskRequest";
	suggested_mode?: string | null;
	suggested_cli_args?: string[] | null;
	reason: string;
}

interface DispatchRunOptions {
	backend: BackendName;
	model?: string;
	taskMode: TaskMode;
	executionMode: ExecutionMode;
	root: string;
	title?: string;
	prompt?: string;
	from?: string;
}

interface DispatchResumeOptions {
	root: string;
	taskId: string;
	message: string;
	executionMode?: ExecutionMode;
}

interface DispatchTemplateOptions {
	root: string;
	kind: TemplateKind;
	output?: string;
}

interface DispatchInvocation {
	command: string;
	argsPrefix: string[];
	cwd: string;
}

let state: DispatchUiState = { root: DEFAULT_ROOT };
let extensionApi: ExtensionAPI | undefined;

export default function dispatchHostExtension(pi: ExtensionAPI) {
	extensionApi = pi;

	pi.registerCommand("dispatch", {
		description: "Run or inspect the Rust dispatch scheduler",
		handler: async (args, ctx) => {
			await handleDispatchCommand(pi, args, ctx);
		},
	});

	pi.registerCommand("dispatch-status", {
		description: "Show status for the last or specified dispatch task",
		handler: async (args, ctx) => {
			const taskId = args.trim() || state.lastTaskId;
			if (!taskId) {
				ctx.ui.notify("No dispatch task selected yet.", "info");
				return;
			}
			await showTaskStatus(pi, ctx, taskId, state.root);
		},
	});

	pi.registerCommand("dispatch-events", {
		description: "Show recent events for the last or specified dispatch task",
		handler: async (args, ctx) => {
			const taskId = args.trim() || state.lastTaskId;
			if (!taskId) {
				ctx.ui.notify("No dispatch task selected yet.", "info");
				return;
			}
			await showTaskEvents(pi, ctx, taskId, state.root);
		},
	});

	pi.registerCommand("dispatch-answer", {
		description: "Resume a task with an answer for the worker",
		handler: async (args, ctx) => {
			const tokens = splitShellArgs(args);
			if (tokens.length < 2) {
				ctx.ui.notify("Usage: /dispatch-answer <task-id> <message...>", "error");
				return;
			}

			const taskId = tokens[0];
			const message = tokens.slice(1).join(" ");
			await resumeTask(pi, ctx, {
				root: state.root,
				taskId,
				message,
			});
		},
	});

	pi.registerCommand("dispatch-questions", {
		description: "Show pending mailbox questions",
		handler: async (args, ctx) => {
			await showTaskQuestions(pi, ctx, args.trim() || undefined, state.root);
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		state = restoreState(ctx) ?? { root: DEFAULT_ROOT };
		await refreshUiFromState(pi, ctx);
	});

	pi.on("session_tree", async (_event, ctx) => {
		state = restoreState(ctx) ?? state;
		await refreshUiFromState(pi, ctx);
	});
}

async function handleDispatchCommand(
	pi: ExtensionAPI,
	rawArgs: string,
	ctx: ExtensionCommandContext,
) {
	const tokens = splitShellArgs(rawArgs);
	if (tokens.length === 0) {
		if (stateHasTask()) {
			await showTaskStatus(pi, ctx, stateTaskId(), stateRoot());
			return;
		}
		await showReady(pi, ctx);
		return;
	}

	const subcommand = tokens[0];
	if (subcommand === "ready") {
		await showReady(pi, ctx);
		return;
	}

	if (subcommand === "status") {
		const taskId = tokens[1] ?? stateTaskId();
		if (!taskId) {
			ctx.ui.notify("No dispatch task selected yet.", "info");
			return;
		}
		await showTaskStatus(pi, ctx, taskId, stateRoot());
		return;
	}

	if (subcommand === "events") {
		const taskId = tokens[1] ?? stateTaskId();
		if (!taskId) {
			ctx.ui.notify("No dispatch task selected yet.", "info");
			return;
		}
		await showTaskEvents(pi, ctx, taskId, stateRoot());
		return;
	}

	if (subcommand === "answer") {
		if (tokens.length < 3) {
			ctx.ui.notify("Usage: /dispatch answer <task-id> <message...>", "error");
			return;
		}
		await resumeTask(pi, ctx, {
			root: stateRoot(),
			taskId: tokens[1],
			message: tokens.slice(2).join(" "),
		});
		return;
	}

	if (subcommand === "questions") {
		await showTaskQuestions(pi, ctx, tokens[1], stateRoot());
		return;
	}

	if (subcommand === "resume") {
		if (tokens.length < 3) {
			ctx.ui.notify("Usage: /dispatch resume <task-id> <message...>", "error");
			return;
		}
		await resumeExecution(pi, ctx, {
			root: stateRoot(),
			taskId: tokens[1],
			message: tokens.slice(2).join(" "),
		});
		return;
	}

	if (subcommand === "config") {
		await runConfigCommand(pi, ctx, tokens.slice(1));
		return;
	}

	if (subcommand === "backends") {
		const result = await runDispatchCli(ctx.cwd, ["backends"]);
		if (!result.ok) {
			ctx.ui.notify(result.error, "error");
			return;
		}
		pi.sendMessage(
			{
				customType: "dispatch-backends",
				content: "```text\n" + result.stdout.trim() + "\n```",
				display: true,
			},
			{ triggerTurn: false },
		);
		return;
	}

	if (subcommand === "template") {
		const options = parseTemplateOptions(tokens.slice(1), state.root, ctx.cwd);
		await generateTemplate(pi, ctx, options);
		return;
	}

	const route = await routeDispatchRequest(ctx.cwd, rawArgs);
	if (route?.kind === "Warmup") {
		await showReady(pi, ctx);
		return;
	}
	if (route?.kind === "ConfigRequest" && route.suggested_cli_args?.length) {
		await runConfigCommand(pi, ctx, route.suggested_cli_args.slice(1));
		return;
	}

	const options = parseRunOptions(tokens, state.root, ctx.cwd);
	if (!options) {
		showUsage(ctx);
		return;
	}
	await runNewTask(pi, ctx, options);
}

async function runNewTask(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	options: DispatchRunOptions,
) {
	const title = options.title ?? deriveTitle(options.prompt ?? options.from ?? "Dispatch Task");
	const args = [
		"run",
		"--backend",
		options.backend,
		"--mode",
		options.taskMode,
		"--execution-mode",
		options.executionMode,
		"--workspace",
		ctx.cwd,
		"--root",
		options.root,
	];
	if (options.title ?? options.prompt) {
		args.push("--title", title);
	}
	if (options.prompt) {
		args.push("--prompt", options.prompt);
	}
	if (options.from) {
		args.push("--from", options.from);
	}

	if (options.model) {
		args.push("--model", options.model);
	}
	const result = await runDispatchCli(ctx.cwd, args);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const payload = parseJson<DispatchExecutionSummary | Record<string, unknown>>(result.stdout);
	if (!payload) {
		ctx.ui.notify("Dispatch returned non-JSON output.", "error");
		return;
	}

	if ("task_id" in payload) {
		await showTaskStatus(pi, ctx, String(payload.task_id), options.root);
		return;
	}

	ctx.ui.notify("Dispatch returned an unexpected payload.", "error");
}

async function generateTemplate(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	options: DispatchTemplateOptions,
) {
	const args = ["template", "--kind", options.kind, "--root", options.root];
	if (options.output) {
		args.push("--output", options.output);
	}
	const result = await runDispatchCli(ctx.cwd, args);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const content = options.output
		? `Template written to \`${result.stdout.trim()}\``
		: "```md\n" + result.stdout.trim() + "\n```";
	pi.sendMessage(
		{
			customType: "dispatch-template",
			content,
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function showReady(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext | ExtensionContext,
) {
	const result = await runDispatchCli(ctx.cwd, ["ready"]);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const payload = parseJson<DispatchReadySummary>(result.stdout);
	if (!payload) {
		ctx.ui.notify("Failed to parse dispatch readiness JSON.", "error");
		return;
	}

	pi.sendMessage(
		{
			customType: "dispatch-ready",
			content: [
				"**Dispatch Ready**",
				`- default: \`${payload.default_target}\``,
				`- backends in config: \`${payload.backend_count}\``,
				`- models in config: \`${payload.model_count}\``,
				`- aliases in config: \`${payload.alias_count}\``,
				`- installed backends: \`${payload.installed_backends.join(", ") || "none"}\``,
				`- config: \`${payload.config_path}\``,
			].join("\n"),
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function routeDispatchRequest(
	cwd: string,
	prompt: string,
): Promise<DispatchRouteSummary | undefined> {
	const result = await runDispatchCli(cwd, ["route", "--prompt", prompt]);
	if (!result.ok) {
		return undefined;
	}
	return parseJson<DispatchRouteSummary>(result.stdout);
}

async function runConfigCommand(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	args: string[],
) {
	if (args.length === 0) {
		const result = await runDispatchCli(ctx.cwd, ["config", "show"]);
		if (!result.ok) {
			ctx.ui.notify(result.error, "error");
			return;
		}
		pi.sendMessage(
			{
				customType: "dispatch-config",
				content: "```yaml\n" + result.stdout.trim() + "\n```",
				display: true,
			},
			{ triggerTurn: false },
		);
		return;
	}

	const result = await runDispatchCli(ctx.cwd, ["config", ...args]);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	pi.sendMessage(
		{
			customType: "dispatch-config",
			content: "```text\n" + result.stdout.trim() + "\n```",
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function resumeTask(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	options: DispatchResumeOptions,
) {
	const result = await runDispatchCli(ctx.cwd, [
		"answer",
		options.taskId,
		"--message",
		options.message,
		"--root",
		options.root,
	]);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const payload = parseJson<DispatchExecutionSummary | Record<string, unknown>>(result.stdout);
	if (!payload) {
		ctx.ui.notify("Dispatch returned non-JSON output.", "error");
		return;
	}

	pi.sendMessage(
		{
			customType: "dispatch-answer",
			content: "```json\n" + JSON.stringify(payload, null, 2) + "\n```",
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function resumeExecution(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	options: DispatchResumeOptions,
) {
	const args = [
		"resume",
		options.taskId,
		"--message",
		options.message,
		"--root",
		options.root,
	];
	if (options.executionMode) {
		args.push("--execution-mode", options.executionMode);
	}

	const result = await runDispatchCli(ctx.cwd, args);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const payload = parseJson<DispatchExecutionSummary | Record<string, unknown>>(result.stdout);
	if (!payload || !("task_id" in payload)) {
		ctx.ui.notify("Dispatch returned an unexpected payload.", "error");
		return;
	}

	await showTaskStatus(pi, ctx, String(payload.task_id), options.root);
}

async function showTaskStatus(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext | ExtensionContext,
	taskId: string,
	root: string,
) {
	const result = await runDispatchCli(ctx.cwd, ["status", taskId, "--root", root]);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const task = parseJson<DispatchTaskRecord>(result.stdout);
	if (!task) {
		ctx.ui.notify("Failed to parse dispatch status JSON.", "error");
		return;
	}

	updateState(pi, {
		root,
		lastTaskId: task.id,
		lastTaskTitle: task.title,
		lastTaskStatus: task.status,
		lastTaskBackend: task.backend,
		lastUpdatedAt: task.updated_at,
	});
	renderUi(ctx, task);

	pi.sendMessage(
		{
			customType: "dispatch-status",
			content: formatTaskMarkdown(task),
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function showTaskEvents(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext | ExtensionContext,
	taskId: string,
	root: string,
) {
	const result = await runDispatchCli(ctx.cwd, ["events", taskId, "--root", root]);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const events = parseJson<DispatchEventRecord[]>(result.stdout);
	if (!events) {
		ctx.ui.notify("Failed to parse dispatch events JSON.", "error");
		return;
	}

	const recent = events.slice(-MAX_EVENT_LINES);
	pi.sendMessage(
		{
			customType: "dispatch-events",
			content: formatEventsMarkdown(taskId, recent, events.length),
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function showTaskQuestions(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext | ExtensionContext,
	taskId: string | undefined,
	root: string,
) {
	const args = ["questions"];
	if (taskId) {
		args.push(taskId);
	}
	args.push("--root", root);

	const result = await runDispatchCli(ctx.cwd, args);
	if (!result.ok) {
		ctx.ui.notify(result.error, "error");
		return;
	}

	const payload = parseJson<unknown[]>(result.stdout);
	if (!payload) {
		ctx.ui.notify("Failed to parse dispatch questions JSON.", "error");
		return;
	}

	pi.sendMessage(
		{
			customType: "dispatch-questions",
			content: "```json\n" + JSON.stringify(payload, null, 2) + "\n```",
			display: true,
		},
		{ triggerTurn: false },
	);
}

async function refreshUiFromState(pi: ExtensionAPI, ctx: ExtensionContext) {
	if (!state.lastTaskId) {
		clearUi(ctx);
		return;
	}

	const result = await runDispatchCli(ctx.cwd, [
		"status",
		state.lastTaskId,
		"--root",
		state.root,
	]);

	if (!result.ok) {
		renderFallbackState(ctx);
		return;
	}

	const task = parseJson<DispatchTaskRecord>(result.stdout);
	if (!task) {
		renderFallbackState(ctx);
		return;
	}

	updateState(pi, {
		root: state.root,
		lastTaskId: task.id,
		lastTaskTitle: task.title,
		lastTaskStatus: task.status,
		lastTaskBackend: task.backend,
		lastUpdatedAt: task.updated_at,
	});
	renderUi(ctx, task);
}

function renderUi(
	ctx: ExtensionCommandContext | ExtensionContext,
	task: DispatchTaskRecord,
) {
	const theme = ctx.ui.theme;
	const statusColor = statusColorName(task.status);
	const shortId = task.id.slice(0, 8);
	const footer =
		theme.fg("accent", "dispatch ") +
		theme.fg(statusColor, task.status.toLowerCase()) +
		theme.fg("dim", ` ${task.backend.toLowerCase()} ${shortId}`);
	ctx.ui.setStatus(STATUS_KEY, footer);
	ctx.ui.setWidget(WIDGET_KEY, [
		theme.bold(task.title),
		`${theme.fg("dim", "task")} ${shortId}`,
		`${theme.fg("dim", "backend")} ${task.backend.toLowerCase()}`,
		`${theme.fg("dim", "status")} ${theme.fg(statusColor, task.status.toLowerCase())}`,
	]);
}

function renderFallbackState(ctx: ExtensionContext) {
	const theme = ctx.ui.theme;
	if (!state.lastTaskId || !state.lastTaskStatus) {
		clearUi(ctx);
		return;
	}
	ctx.ui.setStatus(
		STATUS_KEY,
		theme.fg("accent", "dispatch ") +
			theme.fg(statusColorName(state.lastTaskStatus), state.lastTaskStatus.toLowerCase()) +
			theme.fg("dim", ` ${state.lastTaskId.slice(0, 8)}`),
	);
}

function clearUi(ctx: ExtensionContext) {
	ctx.ui.setStatus(STATUS_KEY, undefined);
	ctx.ui.setWidget(WIDGET_KEY, undefined);
}

function updateState(pi: ExtensionAPI, next: DispatchUiState) {
	state = next;
	pi.appendEntry<DispatchUiState>(STATE_ENTRY_TYPE, state);
}

function restoreState(ctx: ExtensionContext): DispatchUiState | undefined {
	const entries = ctx.sessionManager.getEntries();
	for (let i = entries.length - 1; i >= 0; i--) {
		const entry = entries[i] as {
			type?: string;
			customType?: string;
			data?: DispatchUiState;
		};
		if (entry.type === "custom" && entry.customType === STATE_ENTRY_TYPE) {
			return entry.data;
		}
	}
	return undefined;
}

function parseRunOptions(
	tokens: string[],
	defaultRoot: string,
	cwd: string,
): DispatchRunOptions | undefined {
	let backend: BackendName = DEFAULT_BACKEND;
	let model: string | undefined;
	let taskMode: TaskMode = DEFAULT_TASK_MODE;
	let executionMode: ExecutionMode = DEFAULT_EXECUTION_MODE;
	let root = defaultRoot;
	let title: string | undefined;
	let from: string | undefined;
	const promptParts: string[] = [];

	for (let i = 0; i < tokens.length; i++) {
		const token = tokens[i];
		if (token === "--backend" && tokens[i + 1]) {
			backend = tokens[++i] as BackendName;
			continue;
		}
		if (token === "--model" && tokens[i + 1]) {
			model = tokens[++i];
			continue;
		}
		if (token === "--mode" && tokens[i + 1]) {
			taskMode = tokens[++i] as TaskMode;
			continue;
		}
		if (token === "--execution-mode" && tokens[i + 1]) {
			executionMode = tokens[++i] as ExecutionMode;
			continue;
		}
		if (token === "--root" && tokens[i + 1]) {
			root = tokens[++i];
			continue;
		}
		if (token === "--title" && tokens[i + 1]) {
			title = tokens[++i];
			continue;
		}
		if (token === "--from" && tokens[i + 1]) {
			from = tokens[++i];
			continue;
		}
		promptParts.push(token);
	}

	const prompt = promptParts.join(" ").trim();
	if (!prompt && !from) {
		return undefined;
	}

	return {
		backend,
		model,
		taskMode,
		executionMode,
		root: resolveRoot(root, cwd),
		title,
		prompt: prompt || undefined,
		from: from ? resolveRoot(from, cwd) : undefined,
	};
}

function parseTemplateOptions(
	tokens: string[],
	defaultRoot: string,
	cwd: string,
): DispatchTemplateOptions {
	let kind: TemplateKind = "generic";
	let root = defaultRoot;
	let output: string | undefined;

	for (let i = 0; i < tokens.length; i++) {
		const token = tokens[i];
		if (token === "--kind" && tokens[i + 1]) {
			kind = tokens[++i] as TemplateKind;
			continue;
		}
		if (token === "--root" && tokens[i + 1]) {
			root = tokens[++i];
			continue;
		}
		if (token === "--output" && tokens[i + 1]) {
			output = tokens[++i];
		}
	}

	return {
		root: resolveRoot(root, cwd),
		kind,
		output: output ? resolveRoot(output, cwd) : undefined,
	};
}

async function runDispatchCli(
	cwd: string,
	commandArgs: string[],
): Promise<{ ok: true; stdout: string; stderr: string } | { ok: false; error: string }> {
	const invocation = resolveDispatchInvocation();
	const result = await executeCommand(invocation, [...invocation.argsPrefix, ...commandArgs], cwd);
	if (result.code !== 0) {
		return {
			ok: false,
			error: (result.stderr || result.stdout || "dispatch command failed").trim(),
		};
	}
	return { ok: true, stdout: result.stdout, stderr: result.stderr };
}

async function executeCommand(
	invocation: DispatchInvocation,
	args: string[],
	cwd: string,
): Promise<{ stdout: string; stderr: string; code: number }> {
	const fullCwd = invocation.command === "cargo" ? invocation.cwd : cwd;
	if (extensionApi) {
		return extensionApi.exec(invocation.command, args, { cwd: fullCwd, timeout: 600_000 });
	}

	const { spawn } = await import("node:child_process");
	return await new Promise((resolvePromise) => {
		const proc = spawn(invocation.command, args, {
			cwd: fullCwd,
			shell: false,
			stdio: ["ignore", "pipe", "pipe"],
		});
		let stdout = "";
		let stderr = "";
		proc.stdout.on("data", (data) => {
			stdout += data.toString();
		});
		proc.stderr.on("data", (data) => {
			stderr += data.toString();
		});
		proc.on("close", (code) => {
			resolvePromise({ stdout, stderr, code: code ?? 1 });
		});
	});
}

function resolveDispatchInvocation(): DispatchInvocation {
	const explicitBin = process.env.DISPATCH_BIN;
	if (explicitBin) {
		return { command: explicitBin, argsPrefix: [], cwd: process.cwd() };
	}

	const workspace = findDispatchWorkspace();
	if (workspace) {
		const debugBinary = join(workspace, "target", "debug", "dispatch-cli");
		if (existsSync(debugBinary)) {
			return { command: debugBinary, argsPrefix: [], cwd: workspace };
		}
		return {
			command: "cargo",
			argsPrefix: [
				"run",
				"-q",
				"-p",
				"dispatch-cli",
				"--manifest-path",
				join(workspace, "Cargo.toml"),
				"--",
			],
			cwd: workspace,
		};
	}

	return { command: "dispatch-cli", argsPrefix: [], cwd: process.cwd() };
}

function findDispatchWorkspace(): string | undefined {
	const explicit = process.env.DISPATCH_WORKSPACE;
	if (explicit && existsSync(join(explicit, "Cargo.toml"))) {
		return explicit;
	}

	let current = dirname(fileURLToPath(import.meta.url));
	while (true) {
		if (
			existsSync(join(current, "Cargo.toml")) &&
			existsSync(join(current, "crates", "dispatch-cli", "Cargo.toml"))
		) {
			return current;
		}
		const parent = dirname(current);
		if (parent === current) {
			break;
		}
		current = parent;
	}
	return undefined;
}

function resolveRoot(root: string, cwd: string): string {
	if (root.startsWith("/")) {
		return root;
	}
	return resolve(cwd, root);
}

function deriveTitle(prompt: string): string {
	if (!prompt) return "Dispatch Task";
	const compact = prompt.replace(/\s+/g, " ").trim();
	return compact.length <= 64 ? compact : compact.slice(0, 61) + "...";
}

function formatTaskMarkdown(task: DispatchTaskRecord): string {
	const lines = [
		`**${task.title}**`,
		`- id: \`${task.id}\``,
		`- backend: \`${task.backend.toLowerCase()}\``,
		`- status: \`${task.status.toLowerCase()}\``,
		`- execution: \`${task.execution_mode.toLowerCase()}\``,
	];
	if (task.model) {
		lines.push(`- model: \`${task.model}\``);
	}
	lines.push(`- updated: \`${task.updated_at}\``);
	return lines.join("\n");
}

function formatEventsMarkdown(
	taskId: string,
	events: DispatchEventRecord[],
	totalCount: number,
): string {
	const lines = events.map(
		(event) =>
			`${event.sequence}. \`${event.kind}\` ${event.message} (\`${event.timestamp}\`)`,
	);
	const header =
		totalCount > events.length
			? `**Recent events for \`${taskId}\`** (showing last ${events.length} of ${totalCount})`
			: `**Events for \`${taskId}\`**`;
	return [header, "", ...lines].join("\n");
}

function showUsage(ctx: ExtensionCommandContext) {
	ctx.ui.notify(
		[
			"/dispatch [--backend pi|codex|claude|cursor-agent] [--model MODEL] [--mode auto|direct|plan|discuss] [--execution-mode standard|auto|danger] [--root PATH] [--title TITLE] <prompt>",
			"/dispatch --from plan.md [--backend ...] [--root PATH]",
			"/dispatch template [--kind generic|feature|bugfix|refactor|audit|research] [--output PATH]",
			"/dispatch ready",
			"/dispatch config ...",
			"/dispatch status [task-id]",
			"/dispatch questions [task-id]",
			"/dispatch events [task-id]",
			"/dispatch answer <task-id> <message...>",
			"/dispatch resume <task-id> <message...>",
			"/dispatch backends",
		].join("\n"),
		"info",
	);
}

function splitShellArgs(input: string): string[] {
	const tokens: string[] = [];
	let current = "";
	let quote: "'" | '"' | null = null;
	let escaping = false;

	for (const char of input) {
		if (escaping) {
			current += char;
			escaping = false;
			continue;
		}

		if (char === "\\") {
			escaping = true;
			continue;
		}

		if (quote) {
			if (char === quote) {
				quote = null;
			} else {
				current += char;
			}
			continue;
		}

		if (char === "'" || char === '"') {
			quote = char;
			continue;
		}

		if (/\s/.test(char)) {
			if (current.length > 0) {
				tokens.push(current);
				current = "";
			}
			continue;
		}

		current += char;
	}

	if (current.length > 0) {
		tokens.push(current);
	}
	return tokens;
}

function parseJson<T>(raw: string): T | undefined {
	try {
		return JSON.parse(raw) as T;
	} catch {
		return undefined;
	}
}

function statusColorName(status: string): "success" | "warning" | "error" | "dim" {
	switch (status.toLowerCase()) {
		case "completed":
			return "success";
		case "running":
		case "awaitinguser":
		case "awaiting_user":
			return "warning";
		case "failed":
		case "cancelled":
			return "error";
		default:
			return "dim";
	}
}

function stateHasTask(): boolean {
	return Boolean(state.lastTaskId);
}

function stateTaskId(): string | undefined {
	return state.lastTaskId;
}

function stateRoot(): string {
	return state.root;
}
