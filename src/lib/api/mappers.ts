import { t } from '#lib/i18n/index.js';
import type {
	AddFilesResult,
	AddRejectKind,
	AppError as BackendError,
	AppSettings as BackendSettings,
	ChangeFile,
	ChangeKind,
	ConflictChoice,
	ConflictItem,
	ConflictKind as BackendConflictKind,
	DiffLine,
	FileEntry as BackendFileEntry,
	HistoryItem,
	LoginStart,
	OwnerInfo,
	PointChangeItem,
	ProjectInfo,
	PullResult,
	PushResult as BackendPushResult,
	RemoteConnectResult,
	RemoteProjectList as BackendRemoteProjectList,
	RestoreFilePreviewData,
	RestoreFileResult,
	RestorePreviewData,
	SaveResult,
	SaveSizeChoice,
	SessionInfo,
	SettingsPatch_Deserialize,
	SizeCheckResult,
	SettingsView as BackendSettingsView,
	SyncStatus
} from '#lib/bindings.js';
import { AppError } from './errors';
import type {
	AddFilesOutcome,
	AppSettings,
	Change,
	ChangeType,
	Choice,
	ConflictFile,
	ConflictKind,
	ConflictResolution,
	DeviceFlow,
	DiffRow,
	FetchResult,
	FileDiff,
	FileEntry,
	ImpactItem,
	Owner,
	PointFile,
	Project,
	PushResult,
	RemoteOutcome,
	RejectReason,
	RemoteProjectList,
	SaveOutcome,
	SavePoint,
	SettingKey,
	SettingsView,
	Session,
	SizeCheck,
	SizeChoice
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
		isConflict: file.conflicted || conflictPaths.has(file.path)
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
		remoteConnected: info.remote_url !== null && info.remote_url !== '',
		lastUploadedAt: info.last_uploaded_at === null ? null : parseDate(info.last_uploaded_at),
		unsavedCount: status.unsaved_changes,
		uploadPendingCount: status.upload_pending,
		fetchPendingCount: status.pull_pending,
		hasConflict: status.has_conflicts,
		folderMissing: info.folder_missing || status.folder_missing,
		interruptedOperation: status.interrupted_operation,
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
export function mapSession(info: SessionInfo): Session {
	return {
		loggedIn: info.logged_in,
		onboarded: info.onboarded,
		reauthRequired: info.reauth_required,
		userLogin: info.user?.login ?? null
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
	unreadable: 'unreadable'
};

export function mapAddFilesResult(result: AddFilesResult): AddFilesOutcome {
	return {
		added: result.added.map((item) => ({
			path: item.path.replaceAll('\\', '/'),
			renamed: item.renamed,
			large: item.large
		})),
		rejected: result.rejected.map((item) => ({
			name: item.name,
			reason: REJECT_REASONS[item.reason],
			size: item.size
		}))
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
	autoSaveAfterRestore: 'auto_save_after_restore',
	largeFileWarnMb: 'large_file_warn_mb',
	showSnapshotsInTimeline: 'show_snapshots_in_timeline'
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
		autoSaveAfterRestore: settings.auto_save_after_restore,
		largeFileWarnMb: settings.large_file_warn_mb,
		showSnapshotsInTimeline: settings.show_snapshots_in_timeline
	};
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
