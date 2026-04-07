export function parseJson(raw) {
	try {
		return JSON.parse(raw);
	} catch {
		return undefined;
	}
}

export function parseDispatchSuccessEnvelope(raw) {
	const parsed = parseJson(raw);
	if (!parsed || parsed.ok !== true || !("data" in parsed)) {
		return undefined;
	}
	return parsed;
}

export function parseDispatchErrorEnvelope(raw) {
	const parsed = parseJson(raw);
	if (!parsed || parsed.ok !== false) {
		return undefined;
	}
	return parsed;
}
