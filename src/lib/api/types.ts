import type { AppError } from './errors';

export interface Project {
	id: string;
	name: string;
	path: string;
	ownerName: string;
	ownerKind: 'personal' | 'org';
	lastSavedAt: Date | null;
	/** クラウドの保管場所（GitHub）に接続済みか。false のときは取り込み・アップロードの対象にならない */
	remoteConnected: boolean;
	lastUploadedAt: Date | null;
	unsavedCount: number;
	uploadPendingCount: number;
	fetchPendingCount: number;
	hasConflict: boolean;
	folderMissing: boolean;
	/** 前回のアプリ終了で途中で止まった操作の名前（`save` / `pull` など）。なければ null */
	interruptedOperation: string | null;
}

/** 保存前の検査で見つかった大きいファイル */
export interface LargeFile {
	path: string;
	/** バイト数。取得できなかったときは null */
	sizeBytes: number | null;
}

/** 大きいファイルの検査結果（保存はしていない） */
export interface SizeCheck {
	/** 100MB を超え、保存できないファイル */
	blocked: LargeFile[];
	/** 大きめだが保存できるファイル */
	warned: LargeFile[];
}

/** 大きいファイルについての選択 */
export interface SizeChoice {
	/** 警告だけのファイルをそのまま保存する */
	acceptWarned: boolean;
	/** 保存対象から外すファイルのパス */
	exclude: string[];
}

export type SaveOutcome = { kind: 'saved' } | { kind: 'size_check'; check: SizeCheck };

export type ChangeType = 'modified' | 'added' | 'deleted' | 'renamed';

export interface Change {
	id: string;
	path: string;
	type: ChangeType;
	oldPath?: string;
	isConflict: boolean;
}

export interface SavePoint {
	id: string;
	createdAt: Date;
	message: string;
	kind: 'save' | 'auto';
	cloudSynced: boolean;
	pcName?: string;
}

export interface PointFile {
	path: string;
	type: ChangeType;
	/** 名前変更の場合の元のパス */
	oldPath?: string;
}

/** ある保存時点に存在するファイル（全ファイル表示用） */
export interface FileEntry {
	path: string;
	/** バイト数。取得できない場合は null */
	size: number | null;
}

export type DiffRowKind = 'context' | 'add' | 'del';

export interface DiffRow {
	kind: DiffRowKind;
	oldNo: number | null;
	newNo: number | null;
	text: string;
}

export type FileKind = 'document' | 'table' | 'image' | 'text' | 'pdf' | 'unknown';

export type FileDiff =
	| { kind: 'text'; rows: DiffRow[] }
	| { kind: 'identical' }
	| { kind: 'too_large'; sizeBytes: number }
	| { kind: 'info'; fileKind: FileKind; sizeBytes: number; modifiedAt: Date };

export type ConflictKind = 'both' | 'deleted_in_cloud' | 'deleted_on_this_pc';

export interface ConflictFile {
	path: string;
	kind: ConflictKind;
	/** この PC 側の最終保存日時。取得できない場合は null（画面では非表示） */
	thisPcSavedAt: Date | null;
	/** クラウド側の最終保存日時。取得できない場合は null（画面では非表示） */
	cloudSavedAt: Date | null;
	/** この PC 側を保存した PC の名前。記録が無い場合は null */
	thisPcName: string | null;
	/** クラウド側を保存した PC の名前。記録が無い場合は null */
	cloudPcName: string | null;
}

export type Choice = 'this' | 'cloud';

export interface ConflictResolution {
	path: string;
	choice: Choice;
	keepBoth: boolean;
}

export type RestoreScope = { kind: 'all' } | { kind: 'file'; path: string };

export interface ImpactItem {
	path: string;
	type: 'modified' | 'removed' | 'restored';
}

export interface RestoreResult {
	/** 取り消しに使う復元点。戻していない（取り消すものが無い）場合は null */
	undoToken: string | null;
	changedCount: number;
}

export interface FetchResult {
	mergedCount: number;
	conflictCount: number;
	/** 大きいファイルがあり、何も実行せずに見送った場合の内容。なければ null */
	sizeCheck: SizeCheck | null;
	/**
	 * 未保存の変更があり、取り込む前に保存の確認が必要なため、何も実行せずに見送った場合の
	 * 未保存のファイルの件数（E06）。なければ null
	 */
	unsavedCount: number | null;
}

/** 取り込みの指定 */
export interface FetchOptions {
	/** 取り込む前に未保存の変更を保存することを、利用者が確認で承諾した */
	saveConfirmed?: boolean;
}

export interface PushResult {
	/** 大きいファイルがあり、何も実行せずに見送った場合の内容。なければ null */
	sizeCheck: SizeCheck | null;
}

export interface Owner {
	id: string;
	name: string;
	kind: 'personal' | 'org';
	canCreate: boolean;
}

export interface RemoteProject {
	/** 取得に使う識別子（`所有者/名前`） */
	id: string;
	name: string;
	ownerId: string;
	isPrivate: boolean;
}

export interface RemoteProjectList {
	projects: RemoteProject[];
	/** 件数が多く、一部しか取得していない */
	truncated: boolean;
}

/** GitHub からの取得の段階。割合は取得できない */
export type ClonePhase = 'preparing' | 'downloading' | 'finishing' | 'done' | 'failed';

export type AddProjectMode = 'existing' | 'github' | 'new';
export type Visibility = 'private' | 'public';

export interface AddProjectInput {
	mode: AddProjectMode;
	name: string;
	folder: string;
	ownerId: string;
	visibility: Visibility;
	/** 公開にすることの警告を確認した。`visibility` が公開のときだけ意味を持つ */
	publicConfirmed?: boolean;
	/** 追加と同時に、GitHub に保存先を作って接続する（既存フォルダ・新しく作る場合。ログイン済みのとき） */
	connectCloud?: boolean;
	remoteId?: string;
}

/** 保存先（GitHub 上の場所）を作って接続した結果。途中で失敗しても、ローカルのファイルは無事 */
export interface RemoteOutcome {
	/** GitHub 上に作った保存先の名前（`所有者/名前`）。作れなかったときは null */
	repository: string | null;
	/** 接続できた（以降は取り込み・アップロードの対象になる） */
	connected: boolean;
	/** 最初のアップロードまで完了した */
	uploaded: boolean;
	/**
	 * 最初の保存に大きいファイルがあり、保存を見送った場合の内容。なければ null。
	 * `connected` が false のときは、GitHub に何も作る前に見つかったもの（作成も接続もしていない）
	 */
	sizeCheck: SizeCheck | null;
	/**
	 * 同じ名前の空の保存先（`所有者/名前`）が GitHub にすでにあり、新しくは作らなかった場合の名前。
	 * 何も作らず、接続もしていない。画面が確認し、承認されたら `adoptExisting` で接続をやり直す。なければ null
	 */
	existingEmptyRepository: string | null;
	/** 途中で失敗した場合のエラー（3 要素の文言を持つ）。なければ null */
	error: AppError | null;
}

/** プロジェクト追加の結果。保存先の作成を頼んだ場合だけ `remote` が入る */
export interface AddProjectResult {
	project: Project;
	remote: RemoteOutcome | null;
}

/** 既存のローカルだけのプロジェクトを、GitHub に接続する入力 */
export interface ConnectRemoteInput {
	ownerId: string;
	visibility: Visibility;
	publicConfirmed: boolean;
	/** GitHub 上の名前。空なら自動で決める */
	name?: string;
	/** 同名の空の保存先がすでにあるとき、新しく作らずにそこへ接続する（利用者が確認したあとだけ true） */
	adoptExisting?: boolean;
}

export interface DroppedFile {
	name: string;
	/** バイト数。実パスだけ分かる場合（アプリ上のドロップ）は null */
	size: number | null;
	/** 実際のパス。アプリ上のドロップ・ファイル選択でのみ得られる */
	path?: string;
}

export type RejectReason = 'too_large' | 'not_a_file' | 'unreadable';

export interface AddedFile {
	path: string;
	/** 同名があったため別名にした */
	renamed: boolean;
	/** 大きいファイル。アップロードに時間がかかる */
	large: boolean;
}

export interface RejectedFile {
	name: string;
	reason: RejectReason;
	/** `too_large` のときの元のサイズ（バイト） */
	size: number | null;
}

export interface AddFilesOutcome {
	added: AddedFile[];
	rejected: RejectedFile[];
}

/** 「開く」メニューの選択肢。パスのコピーは画面側（クリップボード）で行い、バックエンドは呼ばない */
export type OpenTarget = 'default' | 'folder' | 'copy_path';

/** バックエンドで実行する「開く」の種類（既定のアプリで開く・フォルダで表示） */
export type OpenAction = Exclude<OpenTarget, 'copy_path'>;

export type ConflictMode = 'show-dialog' | 'notify-only';
export type TimelineSnapshots = 'collapsed' | 'shown' | 'hidden';

/** 画面から変更できる設定（一般・取り込み・アップロード・自動保存） */
export interface AppSettings {
	pullOnStartup: boolean;
	/** 定期的に取り込む間隔（分）。0 はオフ */
	pullIntervalMinutes: number;
	/** 取り込む前に未保存の変更を保存する（false は「確認する」） */
	saveBeforePull: boolean;
	conflictMode: ConflictMode;
	autoPushAfterSave: boolean;
	/** アップロード待ちの通知間隔（時間）。0 はオフ */
	pushReminderHours: number;
	autoSnapshotEnabled: boolean;
	/** 最後の変更からこの秒数だけ静止してから記録する */
	autoSnapshotDelaySecs: number;
	/** 自動保存の保持期間（日） */
	snapshotRetentionDays: number;
	autoSaveAfterRestore: boolean;
	/** 大きいファイルの警告閾値（MB） */
	largeFileWarnMb: number;
	showSnapshotsInTimeline: TimelineSnapshots;
}

export type SettingKey = keyof AppSettings;

/** 設定の値と、プロジェクトを指定した場合にそのプロジェクトで上書きしている項目 */
export interface SettingsView {
	settings: AppSettings;
	overridden: SettingKey[];
}

export interface DeviceFlow {
	/** GitHub の画面で入力するコード */
	userCode: string;
	/** コードを入力するページの URL */
	verificationUri: string;
	/** 有効期限（秒） */
	expiresInSecs: number;
}

export type LoginOutcome = 'succeeded' | 'denied' | 'expired' | 'canceled';

export interface Session {
	loggedIn: boolean;
	onboarded: boolean;
	/** トークンが失効しており、もう一度ログインが必要 */
	reauthRequired: boolean;
	/** ログイン中の GitHub ユーザー名 */
	userLogin: string | null;
}

/** バックエンドが対応している操作。未対応の操作は画面に出さない */
export interface ApiCapabilities {
	/** 1 ファイルだけを過去の版に戻す */
	restoreFile: boolean;
}

export interface ProjectApi {
	readonly capabilities: ApiCapabilities;

	getSession(): Promise<Session>;
	startLogin(): Promise<DeviceFlow>;
	/** ユーザーが GitHub で許可するまで待つ。キャンセルされた場合は `canceled` で戻る */
	waitLogin(): Promise<LoginOutcome>;
	cancelLogin(): Promise<void>;
	/** 進行中のログインの確認ページを既定のブラウザで開く（URL は渡せない。デスクトップアプリのみ） */
	openLoginPage(): Promise<void>;
	completeOnboarding(): Promise<void>;

	listProjects(): Promise<Project[]>;
	getProject(id: string): Promise<Project>;
	listOwners(): Promise<Owner[]>;
	listRemoteProjects(query: string): Promise<RemoteProjectList>;
	/** フォルダ選択。キャンセル時は null */
	pickFolder(): Promise<string | null>;
	/** ファイル選択（アプリ上のみ実パスを得られる）。キャンセル時は null */
	pickFiles(): Promise<string[] | null>;
	/** GitHub から取得する場合、`onProgress` に取得の段階が通知される */
	addProject(
		input: AddProjectInput,
		onProgress?: (phase: ClonePhase) => void
	): Promise<AddProjectResult>;
	/** ローカルだけのプロジェクトを GitHub に接続する。作成前の失敗は例外、作成後のアップロードの失敗は結果の `error` */
	connectRemote(id: string, input: ConnectRemoteInput): Promise<RemoteOutcome>;
	removeProject(id: string): Promise<void>;
	relocateProject(id: string, folder: string): Promise<Project>;

	listChanges(projectId: string): Promise<Change[]>;
	listSavePoints(projectId: string): Promise<SavePoint[]>;
	listPointFiles(projectId: string, savePointId: string): Promise<PointFile[]>;
	/** その時点に存在するすべてのファイル */
	listFilesAt(projectId: string, savePointId: string): Promise<FileEntry[]>;
	/** 未保存の変更から作る保存メモの案（ルールベース） */
	suggestMemo(projectId: string): Promise<string>;
	compare(projectId: string, path: string, fromId: string, toId: string): Promise<FileDiff>;

	/** 大きいファイルがあれば何も保存せず `size_check` で返す。画面は選択を求め、`saveWithSizeChoice` で再実行する */
	save(projectId: string, memo: string): Promise<SaveOutcome>;
	saveWithSizeChoice(projectId: string, memo: string, choice: SizeChoice): Promise<SaveOutcome>;
	restoreImpact(projectId: string, targetId: string, scope: RestoreScope): Promise<ImpactItem[]>;
	restore(projectId: string, targetId: string, scope: RestoreScope): Promise<RestoreResult>;
	undoRestore(projectId: string, undoToken: string): Promise<void>;
	fetch(projectId: string, options?: FetchOptions): Promise<FetchResult>;
	push(projectId: string): Promise<PushResult>;
	listConflicts(projectId: string): Promise<ConflictFile[]>;
	resolveConflicts(projectId: string, resolutions: ConflictResolution[]): Promise<void>;
	abortMerge(projectId: string): Promise<void>;

	addFiles(projectId: string, files: DroppedFile[]): Promise<AddFilesOutcome>;
	openFile(projectId: string, path: string, target: OpenAction): Promise<void>;
	/** 過去の保存時点の版を、読み取り専用で既定のアプリで開く。現在のファイルは変更しない */
	openFileAt(projectId: string, savePointId: string, path: string): Promise<void>;
	/** 中断された操作の直前の状態へ戻す。戻せない場合はエラー（何も変更しない） */
	recoverInterrupted(projectId: string): Promise<void>;

	/** `projectId` を指定すると、そのプロジェクトの上書きを反映した設定を返す */
	getSettings(projectId: string | null): Promise<SettingsView>;
	/** `projectId` を指定すると、そのプロジェクトだけの上書きとして保存する */
	updateSettings(projectId: string | null, patch: Partial<AppSettings>): Promise<SettingsView>;
}
