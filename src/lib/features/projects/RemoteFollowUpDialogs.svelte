<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import SizeCheckDialog from '#lib/features/changes/SizeCheckDialog.svelte';
	import { choiceFor, type SizeAction } from '#lib/features/changes/size-check.js';
	import { useConnectAfterSizeChoice, useConnectRemote } from './mutations';
	import { adoptInput, renamedInput } from './remote-follow-up';
	import {
		clearRemoteFollowUp,
		remoteFollowUp,
		startRemoteFollowUp
	} from './remote-follow-up.svelte';

	// 保存先を作る前に利用者の判断が必要になったときの確認。
	// 同名の空の保存先がある（接続する／別の名前にする／キャンセル）、
	// 最初の保存に大きいファイルがある（既存の大きいファイルの確認を再利用）。
	// どちらも GitHub には何も作っておらず、キャンセルしてもこの PC のファイルは変わらない。
	const current = $derived(remoteFollowUp.current);
	const existing = $derived(
		current && (current.kind === 'existing' || current.kind === 'rename') ? current : null
	);
	const size = $derived(current?.kind === 'size' ? current : null);

	let renaming = $state('');

	// 処理の完了時に、別の続きへ置き換わっていなければ閉じる（置き換わっていればそのまま続ける）
	let startedAt = 0;
	function finish() {
		if (remoteFollowUp.version === startedAt) clearRemoteFollowUp();
	}

	const connect = useConnectRemote(finish, clearRemoteFollowUp);
	const afterSize = useConnectAfterSizeChoice(finish, clearRemoteFollowUp);
	const pending = $derived(connect.isPending || afterSize.isPending);

	$effect(() => {
		if (current?.kind === 'rename') renaming = '';
	});

	const renameError = $derived(
		current?.kind === 'rename' && renaming.trim() === '' ? t('remote_rename.error_name') : undefined
	);
	let submittedRename = $state(false);
	$effect(() => {
		if (current?.kind !== 'rename') submittedRename = false;
	});

	function adopt() {
		if (!existing) return;
		startedAt = remoteFollowUp.version;
		connect.mutate({
			id: existing.projectId,
			name: existing.projectName,
			input: adoptInput(existing.input, existing.repository)
		});
	}

	function startRename() {
		if (!existing) return;
		submittedRename = false;
		startRemoteFollowUp({ ...existing, kind: 'rename' });
	}

	function submitRename() {
		if (current?.kind !== 'rename') return;
		submittedRename = true;
		if (renaming.trim() === '') return;
		startedAt = remoteFollowUp.version;
		connect.mutate({
			id: current.projectId,
			name: current.projectName,
			input: renamedInput(current.input, renaming)
		});
	}

	function chooseSize(action: SizeAction) {
		if (!size) return;
		const choice = choiceFor(size.check, action);
		if (!choice) return;
		startedAt = remoteFollowUp.version;
		afterSize.mutate({ followUp: size, choice });
	}
</script>

<Dialog
	open={current?.kind === 'existing'}
	variant="confirm"
	title={t('remote_existing.title')}
	description={existing ? t('remote_existing.description', { name: existing.repository }) : ''}
	busy={pending}
	onclose={clearRemoteFollowUp}
>
	<p class="m:0 type-body">{t('remote_existing.note')}</p>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('remote_existing.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="ghost" disabled={pending} onclick={clearRemoteFollowUp}>
			{t('remote_existing.cancel')}
		</Button>
		<span class="flex:1"></span>
		<Button variant="secondary" disabled={pending} onclick={startRename}>
			{t('remote_existing.rename')}
		</Button>
		<Button loading={connect.isPending} disabled={pending} onclick={adopt}>
			{t('remote_existing.connect')}
		</Button>
	{/snippet}
</Dialog>

<Dialog
	open={current?.kind === 'rename'}
	variant="form"
	title={t('remote_rename.title')}
	description={t('remote_rename.description')}
	busy={pending}
	onclose={clearRemoteFollowUp}
>
	<form
		class="flex flex-direction:column gap:4"
		onsubmit={(event) => {
			event.preventDefault();
			submitRename();
		}}
	>
		<TextField
			bind:value={renaming}
			label={t('connect.name')}
			placeholder={t('connect.name_placeholder')}
			error={submittedRename ? renameError : undefined}
			maxLength={100}
		/>
	</form>

	{#snippet actions()}
		<span class="flex:1"></span>
		<Button variant="secondary" disabled={pending} onclick={clearRemoteFollowUp}>
			{t('remote_existing.cancel')}
		</Button>
		<Button loading={connect.isPending} disabled={pending} onclick={submitRename}>
			{t('remote_rename.submit')}
		</Button>
	{/snippet}
</Dialog>

<SizeCheckDialog
	check={size?.check ?? null}
	pending={afterSize.isPending}
	onchoose={chooseSize}
	oncancel={clearRemoteFollowUp}
/>
