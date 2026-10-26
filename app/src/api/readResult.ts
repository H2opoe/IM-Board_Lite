import contract from "../../shared/read-result-contract.json";

export const READ_RESULT_MESSAGES = contract.messages;
export const READ_RESULT_ERROR_CODES = contract.errorCodes;

type ReadEnvelope = {
  ok?: boolean;
  data: unknown;
  warnings?: string[];
  error?: { message?: string } | null;
};

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : undefined;
}

function scopes(data: unknown, depth = 0): Record<string, unknown>[] {
  const scope = record(data);
  if (!scope || depth > 8) return [];
  return [scope, ...contract.containerKeys.flatMap(key => scopes(scope[key], depth + 1))];
}

export function readItems(data: unknown, depth = 0): unknown[] {
  if (Array.isArray(data)) return data;
  const scope = record(data);
  if (!scope || depth > 8) throw new Error(contract.messages.invalid);
  for (const key of contract.collectionKeys) {
    if (!(key in scope)) continue;
    if (Array.isArray(scope[key])) return scope[key];
    if (contract.containerKeys.includes(key) && record(scope[key])) continue;
    throw new Error(contract.messages.invalid);
  }
  for (const key of contract.containerKeys) {
    if (record(scope[key]) || Array.isArray(scope[key])) return readItems(scope[key], depth + 1);
  }
  throw new Error(contract.messages.invalid);
}

export function requireCompleteRead(envelope: ReadEnvelope): unknown[] {
  if (envelope.ok === false || envelope.error) throw new Error(envelope.error?.message || contract.messages.api);
  const items = readItems(envelope.data);
  let incomplete = false;
  for (const scope of scopes(envelope.data)) {
    if (scope.error != null) throw new Error(contract.messages.api);
    for (const key of contract.failureCodes) {
      const code = scope[key];
      if (code == null || code === 0 || code === "0") continue;
      if (typeof code !== "number" && typeof code !== "string") throw new Error(contract.messages.invalid);
      throw new Error(contract.messages.api);
    }
    for (const key of contract.successFlags) {
      if (!(key in scope)) continue;
      if (typeof scope[key] !== "boolean") throw new Error(contract.messages.invalid);
      if (!scope[key]) throw new Error(contract.messages.api);
    }
    for (const key of [...contract.completionFlags, ...contract.incompleteFlags]) {
      if (!(key in scope)) continue;
      if (typeof scope[key] !== "boolean") throw new Error(contract.messages.invalid);
      incomplete ||= contract.completionFlags.includes(key) ? !scope[key] : scope[key] === true;
    }
    for (const key of contract.failureLists) {
      if (scope[key] == null) continue;
      if (!Array.isArray(scope[key])) throw new Error(contract.messages.invalid);
      incomplete ||= scope[key].length > 0;
    }
    for (const key of contract.failureCounts) {
      if (!(key in scope)) continue;
      const count = scope[key];
      if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0) throw new Error(contract.messages.invalid);
      incomplete ||= count > 0;
    }
  }
  if (incomplete) throw new Error(contract.messages.incomplete);
  const warnings = envelope.warnings?.filter(warning => warning.trim()) ?? [];
  if (warnings.length) throw new Error(warnings.join("；"));
  return items;
}

export function formatReadSuccess(summary: string, count: number): string {
  return count === 0 ? contract.messages.empty : `${contract.messages.successPrefix}${summary}。`;
}
