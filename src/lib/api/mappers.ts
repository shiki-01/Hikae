import { t } from '#lib/i18n/index.js';
import type {
	AddConflictActionData,
	AddConflictDecisionData,
	AddConflictPolicyData,
	AddFilesResult,
	AddRejectKind,
	AfterRestoreSaveKind,
	AppError as BackendError,
	AppSettings as BackendSettings,
	AppHealth as BackendAppHealth,
	BroadFolderKind as BackendBroadFolderKind,
	FolderCheck as BackendFolderCheck,
	RepairResult as BackendRepairResult,
	FolderWarning as BackendFolderWarning,
	ChangeFile,
	ChangeKind,
	ConflictChoice,
	ConflictItem,
	ConflictKind as BackendConflictKind,
	DiffLine,
	DiffResult,
	OfficeDiff as BackendOfficeDiff,
	OfficeUnavailableReason as BackendOfficeUnavailableReason,
	FilePreview as BackendFilePreview,
	PreviewUnavailableReason as BackendPreviewUnavailableReason,
	ParagraphRow as BackendParagraphRow,
	FileEntry as BackendFileEntry,
	HistoryItem,
	LoginStart,
	OwnerInfo,
	PointChangeItem,
	ProjectFileItem,
	ProjectInfo,
	ProjectTreeData,
	PullResult,
	PushResult as BackendPushResult,
	RemoteConnectResult,
	RemoteProjectList as BackendRemoteProjectList,
	RestoreFilePreviewData,
	RestoreFileResult,
	RestorePreviewData,
	IgnoreKindArg,
	IgnoreRuleItem,
	SkipWorktreeItem,
	SkipStashEntry as BackendSkipStashEntry,
	SkipStashExtracted as BackendSkipStashExtracted,
	SkipStashListView,
	SaveResult,
	SaveSizeChoice,
	SessionInfo,
	SettingsPatch_Deserialize,
	WorkCopyItem as BackendWorkCopyItem,
	SizeCheckResult,
	SettingsView as BackendSettingsView,
	CloudBackupStatus as BackendCloudBackupStatus,
	SyncStatus,
	AiCatalog as BackendAiCatalog,
	ModelView as BackendModelView,
	UnavailableReason as BackendUnavailableReason,
	EndpointCheck as BackendEndpointCheck,
	GenerationTest as BackendGenerationTest,
	LlmDownloadFinished as BackendDownloadFinished,
	LlmDownloadProgress as BackendDownloadProgress
} from '#lib/bindings.js';
import { fileKindOf } from '#lib/utils/file-kind.js';
import { AppError } from './errors';
import type {
	AddConflictAction,
	AddConflictDecision,
	AddConflictPolicy,
	AddFilesOutcome,
	AfterRestoreSave,
	AppSettings,
	AppHealth,
	BroadFolderKind,
	FolderCheck,
	FolderWarning,
	Change,
	ChangeType,
	Choice,
	ConflictFile,
	ConflictEdit,
	ConflictKind,
	ConflictResolution,
	DeviceFlow,
	DiffRow,
	FetchResult,
	FileDiff,
	FileEntry,
	ImpactItem,
	OfficeDiff,
	OfficeUnavailableReason,
	FilePreview,
	PreviewUnavailableReason,
	ParagraphRow,
	Owner,
	PointFile,
	Project,
	ProjectFile,
	ProjectTree,
	PushResult,
	RemoteOutcome,
	RepairOutcome,
	RejectReason,
	RemoteProjectList,
	IgnoreKind,
	IgnoreRule,
	SkipWorktreeEntry,
	SkipStashEntry,
	SkipStashExtracted,
	SkipStashList,
	SaveOutcome,
	SavePoint,
	SettingKey,
	SettingsView,
	Session,
	AiCatalog,
	AiDownloadNotice,
	AiEndpointCheck,
	AiModel,
	AiTestResult,
	AiUnavailableReason,
	SizeCheck,
	SizeChoice,
	SkipReason,
	WorkCopy,
	CloudBackupStatus
} from './types';

/** バックエンドのエラー（3 要素）を画面側の AppError に変換する */
export function mapError(error: BackendError): AppError {
	return new AppError(
		'backend',
		error.technical_info ?? '',
		{},
		{
			code: error.code,
			params: Object.fromEntries(error.params),
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
		isConflict: file.conflicted || conflictPaths.has(file.path),
		untracked: file.untracked
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

/** 競合ファイル。保存日時（Unix 秒）と PC 名は、無ければ null */
export function mapConflict(item: ConflictItem): ConflictFile {
	return {
		path: item.path,
		kind: mapConflictKind(item.kind),
		thisPcSavedAt: fromUnixSeconds(item.this_saved_at),
		cloudSavedAt: fromUnixSeconds(item.cloud_saved_at),
		thisPcName: item.this_pc_name,
		cloudPcName: item.cloud_pc_name
	};
}

export function mapChoice(choice: Choice): ConflictChoice {
	return choice === 'this' ? 'mine' : 'theirs';
}

/** 行ごとの編集（backend の引数はタプルの配列） */
export function mapEdits(edits: ConflictEdit[]): [string, string][] {
	return edits.map((edit) => [edit.path, edit.text]);
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
		cloudSynced: item.cloud_synced,
		...(item.pc_name !== null ? { pcName: item.pc_name } : {})
	};
}

export function mapProject(info: ProjectInfo, status: SyncStatus): Project {
	return {
		id: info.id,
		name: info.display_name,
		path: info.path,
		ownerName: info.owner,
		ownerKind: 'personal',
		lastSavedAt: info.last_saved_at === null ? null : parseDate(info.last_saved_at),
		remoteConnected: info.remote_url !== null && info.remote_url !== '',
		lastUploadedAt: info.last_uploaded_at === null ? null : parseDate(info.last_uploaded_at),
		unsavedCount: status.unsaved_changes,
		uploadPendingCount: status.upload_pending,
		fetchPendingCount: status.pull_pending,
		hasConflict: status.has_conflicts,
		folderMissing: info.folder_missing || status.folder_missing,
		interruptedOperation: status.interrupted_operation,
		repoBroken: status.repo_broken,
		repoLarge: status.repo_large,
		watching: status.watching,
		lastAutoSnapshotAt:
			status.last_auto_snapshot_at === null ? null : parseDate(status.last_auto_snapshot_at)
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

/**
 * 差分の取得結果。新規（未追跡）ファイルは、全行が追加の差分行になる。空のファイル・バイナリ・
 * 大きすぎるファイルは、本文を持たない結果（空 / ファイル情報 / 大きすぎて省略）にする。
 */
/** Office 文書の比較結果を画面の形にする（キーは camelCase、種類の値は画面の表記に合わせる） */
export function mapOfficeDiff(result: BackendOfficeDiff): OfficeDiff {
	switch (result.kind) {
		case 'docx':
			return {
				kind: 'docx',
				rows: result.rows.map(mapParagraphRow),
				truncated: result.truncated
			};
		case 'xlsx':
			return {
				kind: 'xlsx',
				addedSheets: result.added_sheets,
				removedSheets: result.removed_sheets,
				cells: result.cells.map((cell) => ({
					sheet: cell.sheet,
					cell: cell.cell,
					row: cell.row,
					col: cell.col,
					before: cell.before,
					after: cell.after
				})),
				truncated: result.truncated
			};
		case 'unavailable':
			return { kind: 'unavailable', reason: mapOfficeReason(result.reason) };
	}
}

function mapParagraphRow(row: BackendParagraphRow): ParagraphRow {
	switch (row.kind) {
		case 'context':
			return { kind: 'context', oldNo: row.old_no, newNo: row.new_no, text: row.text };
		case 'added':
			return { kind: 'add', newNo: row.new_no, text: row.text };
		case 'removed':
			return { kind: 'del', oldNo: row.old_no, text: row.text };
		case 'omitted':
			return { kind: 'omitted', count: row.count };
	}
}

function mapOfficeReason(reason: BackendOfficeUnavailableReason): OfficeUnavailableReason {
	switch (reason) {
		case 'too-large':
			return 'too_large';
		case 'unsupported-format':
			return 'unsupported_format';
		case 'corrupt':
			return 'corrupt';
	}
}

/** プレビューの結果を画面の形にする（キーは snake_case、値の変換は OfficeDiff と同じ規則） */
export function mapFilePreview(result: BackendFilePreview): FilePreview {
	switch (result.kind) {
		case 'text':
			return { kind: 'text', content: result.content, truncated: result.truncated };
		case 'docx':
			return { kind: 'docx', paragraphs: result.paragraphs, truncated: result.truncated };
		case 'xlsx':
			return {
				kind: 'xlsx',
				sheets: result.sheets.map((sheet) => ({
					name: sheet.name,
					rows: sheet.rows,
					cols: sheet.cols,
					cells: sheet.cells.map((cell) => ({ row: cell.row, col: cell.col, value: cell.value })),
					truncated: sheet.truncated
				}))
			};
		case 'pptx':
			return {
				kind: 'pptx',
				slides: result.slides.map((slide) => ({
					index: slide.index,
					paragraphs: slide.paragraphs
				})),
				truncated: result.truncated
			};
		case 'unavailable':
			return { kind: 'unavailable', reason: mapPreviewReason(result.reason) };
		case 'not-found':
			return { kind: 'not_found' };
	}
}

function mapPreviewReason(reason: BackendPreviewUnavailableReason): PreviewUnavailableReason {
	switch (reason) {
		case 'too-large':
			return 'too_large';
		case 'unsupported-format':
			return 'unsupported_format';
		case 'corrupt':
			return 'corrupt';
		case 'binary':
			return 'binary';
	}
}

export function mapDiffResult(result: DiffResult, path: string): FileDiff {
	switch (result.kind) {
		case 'lines':
			return mapDiff(result.lines);
		case 'new-file-empty':
			return { kind: 'new_empty' };
		case 'binary':
			return {
				kind: 'info',
				fileKind: fileKindOf(path),
				sizeBytes: result.size ?? 0,
				modifiedAt: fromUnixSeconds(result.modified_at)
			};
		case 'too-large':
			return { kind: 'too_large', sizeBytes: result.size ?? 0 };
	}
}

/** プロジェクトフォルダ内のファイル。パスの区切りは `/` に統一する */
export function mapProjectFile(item: ProjectFileItem): ProjectFile {
	return {
		path: item.path.replaceAll('\\', '/'),
		sizeBytes: item.size,
		modifiedAt: fromUnixSeconds(item.modified_at),
		change: item.change === null ? null : mapChangeKind(item.change),
		isConflict: item.conflicted
	};
}

/** プロジェクトフォルダ全体のファイル一覧 */
export function mapProjectTree(data: ProjectTreeData): ProjectTree {
	return {
		files: data.entries.map(mapProjectFile),
		truncated: data.truncated,
		limit: data.limit
	};
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
	return {
		mergedCount: merged ? 1 : 0,
		conflictCount: result.conflicts.length,
		sizeCheck: mapSizeCheck(result.size_check),
		unsavedCount: result.outcome === 'needs-save-confirmation' ? (result.unsaved_count ?? 0) : null
	};
}

/** アップロード結果。大きいファイルで見送った場合だけ、確認が必要な内容を持つ */
export function mapPush(result: BackendPushResult): PushResult {
	return { sizeCheck: mapSizeCheck(result.size_check) };
}

/** 保存時点のファイル一覧。パスの区切りは `/` に統一し、パス順に並べる */
export function mapFileEntries(items: BackendFileEntry[]): FileEntry[] {
	return items
		.map((item) => ({ path: item.path.replaceAll('\\', '/'), size: item.size }))
		.sort((a, b) => a.path.localeCompare(b.path));
}

/** ログイン状態。トークンはバックエンドが返さない */
/** アバター画像として表示してよい（https の）URL だけを返す。それ以外は null */
export function safeAvatarUrl(value: string | null): string | null {
	if (value === null) return null;
	try {
		return new URL(value).protocol === 'https:' ? value : null;
	} catch {
		return null;
	}
}

export function mapSession(info: SessionInfo): Session {
	return {
		loggedIn: info.logged_in,
		onboarded: info.onboarded,
		reauthRequired: info.reauth_required,
		userLogin: info.user?.login ?? null,
		avatarUrl: safeAvatarUrl(info.user?.avatar_url ?? null)
	};
}

export function mapDeviceFlow(start: LoginStart): DeviceFlow {
	return {
		userCode: start.user_code,
		verificationUri: start.verification_uri,
		expiresInSecs: start.expires_in
	};
}

export function mapOwner(info: OwnerInfo): Owner {
	return { id: info.id, name: info.name, kind: info.kind, canCreate: info.can_create };
}

export function mapRemoteProjects(list: BackendRemoteProjectList): RemoteProjectList {
	return {
		projects: list.projects.map((item) => ({
			id: item.id,
			name: item.name,
			ownerId: item.owner_id,
			isPrivate: item.private
		})),
		truncated: list.truncated
	};
}

/** 保存時点で変更されたファイル。名前変更のときだけ元のパスを持つ */
export function mapPointChange(item: PointChangeItem): PointFile {
	return {
		path: item.path,
		type: item.kind,
		...(item.old_path !== null ? { oldPath: item.old_path } : {})
	};
}

/** 1 ファイルだけ戻した場合の影響。変わらない・戻せない場合は空 */
export function mapFileImpact(path: string, preview: RestoreFilePreviewData): ImpactItem[] {
	switch (preview.kind) {
		case 'overwrite':
			return [{ path, type: 'modified' }];
		case 'recreate':
			return [{ path, type: 'restored' }];
		case 'unchanged':
		case 'not-in-that-point':
			return [];
	}
}

/** 1 ファイルを戻せなかった場合のエラー。何も変更していない */
export function restoreFileFailure(
	outcome: Exclude<RestoreFileResult['outcome'], 'restored'>,
	path: string
): AppError {
	const key =
		outcome === 'not-in-that-point' ? 'restore.error_not_in_point' : 'restore.error_ignored';
	return new AppError(
		'backend',
		outcome,
		{},
		{
			code: `restore_file_${outcome}`,
			params: {},
			whatHappened: t(`${key}.title`, { name: path }),
			dataIsSafe: t('restore.error_unchanged'),
			nextAction: t(`${key}.next`)
		}
	);
}

const REJECT_REASONS: Record<AddRejectKind, RejectReason> = {
	'too-large': 'too_large',
	'not-a-file': 'not_a_file',
	unreadable: 'unreadable',
	'in-use': 'in_use'
};

const SKIP_REASONS: Record<'link' | 'hidden' | 'os-temp', SkipReason> = {
	link: 'link',
	hidden: 'hidden',
	'os-temp': 'os_temp'
};

const CONFLICT_POLICIES: Record<AddConflictPolicy, AddConflictPolicyData> = {
	ask: 'ask',
	keep_both: 'keep-both',
	replace: 'replace'
};

const CONFLICT_ACTIONS: Record<AddConflictAction, AddConflictActionData> = {
	replace: 'replace',
	keep_both: 'keep-both',
	skip: 'skip'
};

/** 同名のときの方針をバックエンドの形にする */
export function toBackendConflictPolicy(policy: AddConflictPolicy): AddConflictPolicyData {
	return CONFLICT_POLICIES[policy];
}

/** 追加先のパスごとの選択をバックエンドの形にする。パスは確認の一覧で返ってきた形のまま送る */
export function toBackendConflictDecisions(
	decisions: AddConflictDecision[]
): AddConflictDecisionData[] {
	return decisions.map((decision) => ({
		path: decision.path,
		action: CONFLICT_ACTIONS[decision.action]
	}));
}

export function mapAddFilesResult(result: AddFilesResult): AddFilesOutcome {
	return {
		added: result.added.map((item) => ({
			path: item.path.replaceAll('\\', '/'),
			renamed: item.renamed,
			large: item.large,
			replaced: item.replaced,
			replaceRefused: item.replace_refused
		})),
		rejected: result.rejected.map((item) => ({
			name: item.name,
			reason: REJECT_REASONS[item.reason],
			size: item.size
		})),
		skipped: result.skipped.map((item) => ({
			name: item.name,
			reason: SKIP_REASONS[item.reason]
		})),
		// パスは選択として送り返すため、変換しない
		needsDecision: result.needs_decision.map((item) => ({
			path: item.path,
			canReplace: item.can_replace
		})),
		undoToken: result.undo_token
	};
}

const AFTER_RESTORE_SAVES: Record<AfterRestoreSaveKind, AfterRestoreSave> = {
	'not-requested': 'not_requested',
	saved: 'saved',
	'nothing-to-save': 'nothing_to_save',
	'needs-size-decision': 'needs_size_decision',
	failed: 'failed'
};

/** 元に戻した後の保存の結果 */
export function mapAfterRestoreSave(kind: AfterRestoreSaveKind): AfterRestoreSave {
	return AFTER_RESTORE_SAVES[kind];
}

const BROAD_FOLDER_KINDS: Record<BackendBroadFolderKind, BroadFolderKind> = {
	home: 'home',
	'drive-root': 'drive_root',
	'standard-folder': 'standard_folder',
	'system-folder': 'system_folder'
};

/** 広すぎるフォルダの種類。問題なければ null */
export function mapBroadFolder(kind: BackendBroadFolderKind | null): BroadFolderKind | null {
	return kind === null ? null : BROAD_FOLDER_KINDS[kind];
}

const FOLDER_WARNINGS: Record<BackendFolderWarning, FolderWarning> = {
	'cloud-sync': 'cloud_sync'
};

/** フォルダの確認結果 */
export function mapFolderCheck(check: BackendFolderCheck): FolderCheck {
	return {
		broad: mapBroadFolder(check.broad),
		warnings: check.warnings.map((warning) => FOLDER_WARNINGS[warning])
	};
}

/** 破損リポジトリの「取得し直す」の結果 */
export function mapRepairResult(result: BackendRepairResult): RepairOutcome {
	return {
		reason: result.reason,
		asidePath: result.aside_path,
		copied: result.copied,
		failed: result.failed
	};
}

/** アプリ全体の健全性 */
export function mapAppHealth(health: BackendAppHealth): AppHealth {
	return {
		partsAvailable: health.git_available,
		partsSource: health.git_source,
		partsProblem:
			health.git_problem === null
				? null
				: health.git_problem === 'not-found'
					? 'not_found'
					: 'broken'
	};
}

/** 保存の結果。大きいファイルがあって保存しなかったときだけ、確認が必要な結果にする。パスは選択として送り返すため、変換しない */
export function mapSaveResult(result: SaveResult): SaveOutcome {
	const check = mapSizeCheck(result.size_check);
	return check === null ? { kind: 'saved' } : { kind: 'size_check', check };
}

/** 大きいファイルの検査結果。なし・該当ファイルなしは null。パスは変換しない */
export function mapSizeCheck(check: SizeCheckResult | null): SizeCheck | null {
	if (check === null || (check.blocked.length === 0 && check.warned.length === 0)) return null;
	const convert = (items: SizeCheckResult['blocked']) =>
		items.map((item) => ({ path: item.path, sizeBytes: item.size }));
	return { blocked: convert(check.blocked), warned: convert(check.warned) };
}

/** 大きいファイルについての選択をバックエンドの形にする。パスは検査で返ってきた形のまま渡す */
export function toBackendSizeChoice(choice: SizeChoice): SaveSizeChoice {
	return { accept_warned: choice.acceptWarned, exclude: choice.exclude };
}

/** 保存しないファイルの 1 行をバックエンドから受け取った形にする */
export function mapIgnoreRule(item: IgnoreRuleItem): IgnoreRule {
	return { pattern: item.pattern, trackedCount: item.tracked_count };
}

/** 「今後の変更だけ無視する」の記録 1 件（バックエンドの形から画面の形へ） */
export function mapSkipWorktreeEntry(item: SkipWorktreeItem): SkipWorktreeEntry {
	return {
		path: item.path,
		setAt: item.set_at,
		ignored: item.ignored,
		dismissed: item.dismissed
	};
}

/** 退避に残っているファイル 1 件（バックエンドの形から画面の形へ。理由の文字列は同じ） */
export function mapSkipStashEntry(item: BackendSkipStashEntry): SkipStashEntry {
	return {
		id: item.id,
		path: item.path,
		sizeBytes: item.size_bytes ?? 0,
		savedAt: item.saved_at ?? null,
		reason: item.reason
	};
}

/** 退避の一覧（壊れていて一覧に出していない件数を含む） */
export function mapSkipStashList(list: SkipStashListView): SkipStashList {
	return {
		items: list.items.map(mapSkipStashEntry),
		skippedInvalid: list.skipped_invalid
	};
}

/** 退避の取り出し結果 */
export function mapSkipStashExtracted(result: BackendSkipStashExtracted): SkipStashExtracted {
	return { copyPath: result.copy_path };
}

/** 保存しないファイルの種類をバックエンドの形にする（文字列は同じ） */
export function toBackendIgnoreKind(kind: IgnoreKind): IgnoreKindArg {
	return kind;
}

/** 画面側の設定名とバックエンドの項目名の対応 */
export const SETTING_BACKEND_KEYS = {
	pullOnStartup: 'pull_on_startup',
	pullIntervalMinutes: 'pull_interval_minutes',
	saveBeforePull: 'save_before_pull',
	conflictMode: 'conflict_mode',
	autoPushAfterSave: 'auto_push_after_save',
	pushReminderHours: 'push_reminder_hours',
	autoSnapshotEnabled: 'auto_snapshot_enabled',
	autoSnapshotDelaySecs: 'auto_snapshot_delay_secs',
	snapshotRetentionDays: 'snapshot_retention_days',
	snapshotSizeCapX: 'snapshot_size_cap_x',
	snapshotCloudBackup: 'snapshot_cloud_backup',
	autoSaveAfterRestore: 'auto_save_after_restore',
	largeFileWarnMb: 'large_file_warn_mb',
	showSnapshotsInTimeline: 'show_snapshots_in_timeline',
	memoSuggestion: 'memo_suggestion',
	aiRunner: 'ai_runner',
	aiModel: 'ai_model',
	aiModelSource: 'ai_model_source',
	aiCustomGgufPath: 'ai_custom_gguf_path',
	aiScope: 'ai_scope',
	aiSuggestBody: 'ai_suggest_body',
	aiOllamaUrl: 'ai_ollama_url',
	extensionDevMode: 'extension_dev_mode',
	workCopyEnabled: 'work_copy_enabled',
	language: 'language'
} as const satisfies Record<SettingKey, keyof BackendSettings>;

export function mapSettings(settings: BackendSettings): AppSettings {
	return {
		pullOnStartup: settings.pull_on_startup,
		pullIntervalMinutes: settings.pull_interval_minutes,
		saveBeforePull: settings.save_before_pull,
		conflictMode: settings.conflict_mode,
		autoPushAfterSave: settings.auto_push_after_save,
		pushReminderHours: settings.push_reminder_hours,
		autoSnapshotEnabled: settings.auto_snapshot_enabled,
		autoSnapshotDelaySecs: settings.auto_snapshot_delay_secs,
		snapshotRetentionDays: settings.snapshot_retention_days,
		snapshotSizeCapX: settings.snapshot_size_cap_x,
		snapshotCloudBackup: settings.snapshot_cloud_backup,
		autoSaveAfterRestore: settings.auto_save_after_restore,
		largeFileWarnMb: settings.large_file_warn_mb,
		showSnapshotsInTimeline: settings.show_snapshots_in_timeline,
		memoSuggestion: settings.memo_suggestion,
		aiRunner: settings.ai_runner,
		aiModel: settings.ai_model,
		aiModelSource: settings.ai_model_source,
		aiCustomGgufPath: settings.ai_custom_gguf_path,
		aiScope: settings.ai_scope,
		aiSuggestBody: settings.ai_suggest_body,
		aiOllamaUrl: settings.ai_ollama_url,
		extensionDevMode: settings.extension_dev_mode,
		workCopyEnabled: settings.work_copy_enabled,
		language: settings.language
	};
}

/** 自動保存のクラウド控えの状態（件数は数値。未確認は null のまま） */
export function mapCloudBackupStatus(status: BackendCloudBackupStatus): CloudBackupStatus {
	return {
		enabled: status.enabled,
		localCount: status.local_count,
		cloudCount: status.cloud_count
	};
}

/** 作業コピーの一覧の 1 件（作成時刻は RFC 3339 の文字列で届く） */
export function mapWorkCopy(item: BackendWorkCopyItem): WorkCopy {
	return {
		branch: item.branch,
		displayName: item.display_name,
		isCurrent: item.is_current,
		isOriginal: item.is_original,
		archived: item.archived,
		createdAt: item.created_at ? new Date(item.created_at) : null
	};
}

/** 導入できない理由を画面の形にする（数値が無いときは 0） */
export function mapAiUnavailable(
	reason: BackendUnavailableReason | null
): AiUnavailableReason | null {
	if (!reason) return null;
	const needed = reason.needed ?? 0;
	const have = reason.have ?? 0;
	return reason.kind === 'not-enough-memory'
		? { kind: 'memory', needed, have }
		: { kind: 'disk', needed, have };
}

function mapAiModel(model: BackendModelView): AiModel {
	return {
		id: model.id,
		displayName: model.display_name,
		stage: model.stage,
		sizeBytes: model.size_bytes ?? 0,
		licenseName: model.license_name,
		licenseUrl: model.license_url,
		minMemoryBytes: model.min_memory_bytes ?? 0,
		recommended: model.recommended,
		recommendedForThisPc: model.recommended_for_this_pc,
		status: model.status,
		partialBytes: model.partial_bytes ?? 0,
		available: model.available,
		unavailableReason: mapAiUnavailable(model.unavailable_reason),
		downloading: model.downloading
	};
}

/** AI タブの一覧 */
export function mapAiCatalog(catalog: BackendAiCatalog): AiCatalog {
	return {
		system: {
			totalMemoryBytes: catalog.system.total_memory_bytes ?? 0,
			freeDiskBytes: catalog.system.free_disk_bytes ?? 0
		},
		models: catalog.models.map(mapAiModel)
	};
}

export function mapEndpointCheck(check: BackendEndpointCheck): AiEndpointCheck {
	return { ok: check.ok, externalHost: check.external_host };
}

export function mapTestResult(result: BackendGenerationTest): AiTestResult {
	return { ok: result.ok, reason: result.reason };
}

/** ダウンロードの通知（進捗・終了）を画面の形にする */
export function mapDownloadProgress(event: BackendDownloadProgress): AiDownloadNotice {
	return {
		kind: 'progress',
		id: event.id,
		received: event.received ?? 0,
		total: event.total ?? 0
	};
}

export function mapDownloadFinished(event: BackendDownloadFinished): AiDownloadNotice {
	return { kind: 'finished', id: event.id, outcome: event.outcome };
}

/** 上書きしている項目のキー名（バックエンド側）を画面側の名前にする。画面で扱わない項目は除く */
export function mapOverridden(keys: string[]): SettingKey[] {
	const entries = Object.entries(SETTING_BACKEND_KEYS) as [SettingKey, string][];
	return entries.filter(([, backend]) => keys.includes(backend)).map(([key]) => key);
}

export function mapSettingsView(view: BackendSettingsView): SettingsView {
	return { settings: mapSettings(view.settings), overridden: mapOverridden(view.overridden) };
}

/** 画面側の変更内容を、指定された項目だけのバックエンド用の更新内容にする */
export function toBackendPatch(patch: Partial<AppSettings>): SettingsPatch_Deserialize {
	const result: Record<string, unknown> = {};
	for (const key of Object.keys(patch) as SettingKey[]) {
		const value = patch[key];
		if (value !== undefined) result[SETTING_BACKEND_KEYS[key]] = value;
	}
	return result as SettingsPatch_Deserialize;
}

/** 保存先の作成・接続の結果。途中で失敗した場合のエラーは、画面で 3 要素の文言として表示できる形にする */
export function mapRemoteOutcome(result: RemoteConnectResult): RemoteOutcome {
	return {
		repository: result.repository,
		connected: result.connected,
		uploaded: result.uploaded,
		sizeCheck: mapSizeCheck(result.size_check),
		existingEmptyRepository: result.existing_empty_repository,
		error: result.error === null ? null : mapError(result.error)
	};
}
