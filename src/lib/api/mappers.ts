import type {
	AppError as BackendError,
	ChangeFile,
	ChangeKind,
	ConflictChoice,
	ConflictItem,
	ConflictKind as BackendConflictKind,
	DiffLine,
	HistoryItem,
	ProjectInfo,
	PullResult,
	RestorePreviewData,
	SyncStatus
} from '#lib/bindings.js';
import { AppError } from './errors';
import type {
	Change,
	ChangeType,
	Choice,
	ConflictFile,
	ConflictKind,
	ConflictResolution,
	DiffRow,
	FetchResult,
	FileDiff,
	ImpactItem,
	Project,
	SavePoint
} from './types';

/** バックエンドのエラー（3 要素）を画面側の AppError に変換する */
export function mapError(error: BackendError): AppError {
	return new AppError(
		'backend',
		error.technical_info ?? '',
		{},
		{
			whatHappened: error.what_happened,
			dataIsSafe: error.data_is_safe,
			nextAction: error.next_action
		}
	);
}

/** ISO 8601 文字列を Date に変換する。解釈できない場合は null */
export function parseDate(value: string): Date | null {
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? null : date;
}

/** Unix 秒を Date に変換する。null・非有限値・0 以下は null（日時を表示しない） */
export function fromUnixSeconds(value: number | null): Date | null {
	if (value === null || !Number.isFinite(value) || value <= 0) return null;
	return new Date(value * 1000);
}

export function mapChangeKind(kind: ChangeKind): ChangeType {
	return kind;
}

export function mapChange(file: ChangeFile, conflictPaths: ReadonlySet<string>): Change {
	return {
		id: file.path,
		path: file.path,
		type: mapChangeKind(file.kind),
		isConflict: conflictPaths.has(file.path)
	};
}

export function mapConflictKind(kind: BackendConflictKind): ConflictKind {
	switch (kind) {
		case 'deleted-by-us':
			return 'deleted_on_this_pc';
		case 'deleted-by-them':
			return 'deleted_in_cloud';
		case 'both-modified':
		case 'both-added':
		case 'both-deleted':
			return 'both';
	}
}

/** 競合ファイル。保存日時（Unix 秒）が無ければ null、PC 名はバックエンドが返さないため常に null */
export function mapConflict(item: ConflictItem): ConflictFile {
	return {
		path: item.path,
		kind: mapConflictKind(item.kind),
		thisPcSavedAt: fromUnixSeconds(item.this_saved_at),
		cloudSavedAt: fromUnixSeconds(item.cloud_saved_at),
		cloudPcName: null
	};
}

export function mapChoice(choice: Choice): ConflictChoice {
	return choice === 'this' ? 'mine' : 'theirs';
}

export function mapResolutions(resolutions: ConflictResolution[]): {
	choices: [string, ConflictChoice][];
	keepOtherCopy: boolean;
} {
	return {
		choices: resolutions.map((r) => [r.path, mapChoice(r.choice)]),
		keepOtherCopy: resolutions.some((r) => r.keepBoth)
	};
}

export function mapSavePoint(item: HistoryItem): SavePoint {
	return {
		id: item.commit,
		createdAt: parseDate(item.timestamp) ?? new Date(0),
		message: item.message,
		kind: item.is_snapshot ? 'auto' : 'save',
		cloudSynced: false
	};
}

export function mapProject(
	info: ProjectInfo,
	status: SyncStatus,
	lastSavedAt: Date | null
): Project {
	return {
		id: info.id,
		name: info.display_name,
		path: info.path,
		ownerName: info.owner,
		ownerKind: 'personal',
		lastSavedAt,
		lastUploadedAt: null,
		unsavedCount: status.unsaved_changes,
		uploadPendingCount: status.upload_pending,
		fetchPendingCount: status.pull_pending,
		hasConflict: status.has_conflicts,
		folderMissing: false
	};
}

/** 差分行に旧・新の行番号を振る。差分が空なら同一扱い */
export function mapDiff(lines: DiffLine[]): FileDiff {
	if (lines.length === 0) return { kind: 'identical' };
	let oldNo = 1;
	let newNo = 1;
	const rows: DiffRow[] = lines.map((line) => {
		switch (line.kind) {
			case 'added':
				return { kind: 'add', oldNo: null, newNo: newNo++, text: line.content };
			case 'removed':
				return { kind: 'del', oldNo: oldNo++, newNo: null, text: line.content };
			case 'context':
				return { kind: 'context', oldNo: oldNo++, newNo: newNo++, text: line.content };
		}
	});
	return { kind: 'text', rows };
}

export function mapImpact(preview: RestorePreviewData): ImpactItem[] {
	return [
		...preview.modified.map((path): ImpactItem => ({ path, type: 'modified' })),
		...preview.deleted.map((path): ImpactItem => ({ path, type: 'removed' })),
		...preview.created.map((path): ImpactItem => ({ path, type: 'restored' }))
	];
}

/** 取り込み結果。マージ件数はバックエンドが返さないため、取り込みの有無を 0 / 1 で表す */
export function mapPull(result: PullResult): FetchResult {
	const merged = result.outcome === 'merged' || result.outcome === 'fast-forwarded';
	return { mergedCount: merged ? 1 : 0, conflictCount: result.conflicts.length };
}
