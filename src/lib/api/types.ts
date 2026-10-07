export interface Project {
	id: string;
	name: string;
	path: string;
	ownerName: string;
	ownerKind: 'personal' | 'org';
	lastSavedAt: Date | null;
	lastUploadedAt: Date | null;
	unsavedCount: number;
	uploadPendingCount: number;
	fetchPendingCount: number;
	hasConflict: boolean;
	folderMissing: boolean;
}

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
	type: Exclude<ChangeType, 'renamed'>;
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
	thisPcSavedAt: Date;
	cloudSavedAt: Date;
	cloudPcName: string;
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
	undoToken: string;
	changedCount: number;
}

export interface FetchResult {
	mergedCount: number;
	conflictCount: number;
}

export interface Owner {
	id: string;
	name: string;
	kind: 'personal' | 'org';
	canCreate: boolean;
}

export interface RemoteProject {
	id: string;
	name: string;
	ownerId: string;
}

export type AddProjectMode = 'existing' | 'github' | 'new';
export type Visibility = 'private' | 'public';

export interface AddProjectInput {
	mode: AddProjectMode;
	name: string;
	folder: string;
	ownerId: string;
	visibility: Visibility;
	remoteId?: string;
}

export interface DroppedFile {
	name: string;
	size: number;
}

export type OpenTarget = 'default' | 'vscode' | 'folder' | 'copy_path';

export interface AppSettings {
	autoSaveBeforeFetch: boolean;
	autoFetchOnLaunch: boolean;
	autoUploadOnSave: boolean;
	autoSaveAfterRestore: boolean;
}

export interface DeviceFlow {
	userCode: string;
}

export interface Session {
	loggedIn: boolean;
	onboarded: boolean;
}

export interface ProjectApi {
	getSession(): Promise<Session>;
	startLogin(): Promise<DeviceFlow>;
	waitLogin(): Promise<void>;
	completeOnboarding(): Promise<void>;

	listProjects(): Promise<Project[]>;
	getProject(id: string): Promise<Project>;
	listOwners(): Promise<Owner[]>;
	listRemoteProjects(query: string): Promise<RemoteProject[]>;
	pickFolder(): Promise<string>;
	addProject(input: AddProjectInput): Promise<Project>;
	removeProject(id: string): Promise<void>;
	relocateProject(id: string, folder: string): Promise<Project>;

	listChanges(projectId: string): Promise<Change[]>;
	listSavePoints(projectId: string): Promise<SavePoint[]>;
	listPointFiles(projectId: string, savePointId: string): Promise<PointFile[]>;
	compare(projectId: string, path: string, fromId: string, toId: string): Promise<FileDiff>;

	save(projectId: string, memo: string): Promise<void>;
	restoreImpact(projectId: string, targetId: string, scope: RestoreScope): Promise<ImpactItem[]>;
	restore(projectId: string, targetId: string, scope: RestoreScope): Promise<RestoreResult>;
	undoRestore(projectId: string, undoToken: string): Promise<void>;
	fetch(projectId: string): Promise<FetchResult>;
	push(projectId: string): Promise<void>;
	listConflicts(projectId: string): Promise<ConflictFile[]>;
	resolveConflicts(projectId: string, resolutions: ConflictResolution[]): Promise<void>;
	abortMerge(projectId: string): Promise<void>;

	addFiles(projectId: string, files: DroppedFile[]): Promise<number>;
	openFile(projectId: string, path: string, target: OpenTarget): Promise<void>;

	getSettings(): Promise<AppSettings>;
	updateSettings(patch: Partial<AppSettings>): Promise<AppSettings>;
}
