import test from "node:test";
import assert from "node:assert/strict";

import {
	parseDispatchErrorEnvelope,
	parseDispatchSuccessEnvelope,
	parseJson,
} from "./json-envelope.js";

test("parseJson returns undefined for invalid JSON", () => {
	assert.equal(parseJson("{not-json}"), undefined);
});

test("parseDispatchSuccessEnvelope returns data for success envelopes", () => {
	const payload = parseDispatchSuccessEnvelope(
		JSON.stringify({
			ok: true,
			data: {
				task_id: "123",
				status: "dispatched",
			},
		}),
	);

	assert.deepEqual(payload, {
		ok: true,
		data: {
			task_id: "123",
			status: "dispatched",
		},
	});
});

test("parseDispatchSuccessEnvelope rejects non-envelope payloads", () => {
	assert.equal(parseDispatchSuccessEnvelope(JSON.stringify({ task_id: "123" })), undefined);
	assert.equal(parseDispatchSuccessEnvelope(JSON.stringify({ ok: false })), undefined);
});

test("parseDispatchErrorEnvelope returns error payloads", () => {
	const payload = parseDispatchErrorEnvelope(
		JSON.stringify({
			ok: false,
			error: {
				message: "task does not exist",
			},
		}),
	);

	assert.deepEqual(payload, {
		ok: false,
		error: {
			message: "task does not exist",
		},
	});
});

test("parseDispatchErrorEnvelope rejects success payloads", () => {
	assert.equal(parseDispatchErrorEnvelope(JSON.stringify({ ok: true, data: {} })), undefined);
});
