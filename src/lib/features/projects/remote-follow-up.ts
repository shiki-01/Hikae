import type {
	AddProjectInput,
	ConnectRemoteInput,
	RemoteOutcome,
	SizeCheck
} from '#lib/api/types.js';

/**
 * GitHub に保存先を作る前に、利用者の判断が必要になったときの続き。
 * どちらの場合も GitHub には何も作っておらず、プロジェクトの接続も変えていない。
 */
export type RemoteFollowUp =
	| {
			/** 同じ名前の空の保存先がすでにある。「ここに接続しますか」と確認する */
			kind: 'existing';
			projectId: string;
			projectName: string;
			input: ConnectRemoteInput;
			/** 既存の保存先（`所有者/名前`） */
			repository: string;
	  }
	| {
			/** 「別の名前にする」を選んだ。名前を入力してもらう */
			kind: 'rename';
			projectId: string;
			projectName: string;
			input: ConnectRemoteInput;
			repository: string;
	  }
	| {
			/** 最初の保存に大きいファイルがある。選択を保存に反映してから接続をやり直す */
			kind: 'size';
			projectId: string;
			projectName: string;
			input: ConnectRemoteInput;
			check: SizeCheck;
	  };

export interface FollowUpContext {
	projectId: string;
	projectName: string;
	/** 接続をやり直すときの入力 */
	input: ConnectRemoteInput;
}

/** 結果が利用者の判断を求めているときだけ、続きの内容にする。なければ null */
export function followUpFor(
	outcome: RemoteOutcome | null,
	context: FollowUpContext
): RemoteFollowUp | null {
	if (outcome === null || outcome.connected || outcome.error) return null;
	if (outcome.existingEmptyRepository) {
		return { kind: 'existing', repository: outcome.existingEmptyRepository, ...context };
	}
	if (outcome.sizeCheck) return { kind: 'size', check: outcome.sizeCheck, ...context };
	return null;
}

/** 保存先の名前（`所有者/名前` の名前の部分） */
export function repositoryName(repository: string): string {
	return repository.slice(repository.indexOf('/') + 1);
}

/** 承認された既存の空の保存先へ接続するための入力。名前を既存のものにそろえる */
export function adoptInput(input: ConnectRemoteInput, repository: string): ConnectRemoteInput {
	return { ...input, name: repositoryName(repository), adoptExisting: true };
}

/** 別の名前で新しく作るための入力。既存の保存先には接続しない */
export function renamedInput(input: ConnectRemoteInput, name: string): ConnectRemoteInput {
	return { ...input, name: name.trim(), adoptExisting: false };
}

/** プロジェクトの追加時の入力から、あとで接続をやり直すための入力を作る */
export function connectInputFromAdd(input: AddProjectInput): ConnectRemoteInput {
	return {
		ownerId: input.ownerId,
		visibility: input.visibility,
		publicConfirmed: input.visibility === 'public' && input.publicConfirmed === true
	};
}
