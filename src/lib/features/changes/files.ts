import type { DroppedFile } from '#lib/api/types.js';

export const MAX_FILE_BYTES = 100 * 1024 * 1024;

export interface DropClassification {
	accepted: DroppedFile[];
	rejected: DroppedFile[];
}

export function classifyDropped(
	files: DroppedFile[],
	limit: number = MAX_FILE_BYTES
): DropClassification {
	const accepted: DroppedFile[] = [];
	const rejected: DroppedFile[] = [];
	for (const file of files) {
		(file.size > limit ? rejected : accepted).push(file);
	}
	return { accepted, rejected };
}
