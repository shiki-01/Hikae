import { describe, expect, it } from 'vitest';
import type { AddProjectInput, ConnectRemoteInput, RemoteOutcome } from '#lib/api/types.js';
import {
	adoptInput,
	connectInputFromAdd,
	followUpFor,
	renamedInput,
	repositoryName
} from './remote-follow-up';

const input: ConnectRemoteInput = { ownerId: 'me', visibility: 'private', publicConfirmed: false };
const context = { projectId: 'p1', projectName: '卒業論文', input };

const base: RemoteOutcome = {
	repository: null,
	connected: false,
	uploaded: false,
	sizeCheck: null,
	existingEmptyRepository: null,
	error: null
};

describe('保存先の作成の続き', () => {
	it('同名の空の保存先があるときは、接続の確認にする', () => {
		const followUp = followUpFor({ ...base, existingEmptyRepository: 'me/thesis' }, context);
		expect(followUp).toMatchObject({ kind: 'existing', repository: 'me/thesis', projectId: 'p1' });
	});

	it('作成の前に大きいファイルが見つかったときは、サイズの確認にする', () => {
		const check = { blocked: [], warned: [{ path: 'a.psd', sizeBytes: 72 }] };
		expect(followUpFor({ ...base, sizeCheck: check }, context)).toMatchObject({
			kind: 'size',
			check
		});
	});

	it('接続できた・失敗した・何も無い結果は続きにしない', () => {
		const check = { blocked: [], warned: [{ path: 'a.psd', sizeBytes: 72 }] };
		// 接続できたあとの大きいファイル（最初の保存の見送り）は、通知だけで確認は出さない
		expect(followUpFor({ ...base, connected: true, sizeCheck: check }, context)).toBeNull();
		expect(followUpFor({ ...base, connected: true, uploaded: true }, context)).toBeNull();
		expect(followUpFor(null, context)).toBeNull();
	});

	it('承認すると、既存の保存先の名前で接続し直す（新しくは作らない設定）', () => {
		expect(adoptInput(input, 'me/thesis')).toEqual({
			...input,
			name: 'thesis',
			adoptExisting: true
		});
		expect(repositoryName('me/thesis')).toBe('thesis');
	});

	it('別の名前にすると、既存の保存先には接続せず、前後の空白を除いた名前で作る', () => {
		const renamed = renamedInput({ ...input, adoptExisting: true, name: 'thesis' }, '  thesis-2 ');
		expect(renamed).toEqual({ ...input, name: 'thesis-2', adoptExisting: false });
	});

	it('追加時の入力から、接続のやり直しの入力を作る（公開は確認済みのときだけ）', () => {
		const add: AddProjectInput = {
			mode: 'existing',
			name: '卒業論文',
			folder: 'C:/x',
			ownerId: 'me',
			visibility: 'public',
			publicConfirmed: true,
			connectCloud: true
		};
		expect(connectInputFromAdd(add)).toEqual({
			ownerId: 'me',
			visibility: 'public',
			publicConfirmed: true
		});
		expect(connectInputFromAdd({ ...add, visibility: 'private' }).publicConfirmed).toBe(false);
	});
});
