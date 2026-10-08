import type { DroppedFile } from '#lib/api/types.js';

export const MAX_FILE_BYTES = 100 * 1024 * 1024;
/** この大きさ以上のファイルは、アップロードに時間がかかることを知らせる */
export const LARGE_WARN_BYTES = 50 * 1024 * 1024;

export interface DropClassification {
	accepted: DroppedFile[];
	rejected: DroppedFile[];
}

/** サイズが分かるファイルのうち、上限を超えるものを分ける（サイズ不明のものは受け付ける） */
export function classifyDropped(
	files: DroppedFile[],
	limit: number = MAX_FILE_BYTES
): DropClassification {
	const accepted: DroppedFile[] = [];
	const rejected: DroppedFile[] = [];
	for (const file of files) {
		(file.size !== null && file.size > limit ? rejected : accepted).push(file);
	}
	return { accepted, rejected };
}
