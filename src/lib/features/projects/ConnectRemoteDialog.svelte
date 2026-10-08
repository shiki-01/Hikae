<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { RemoteOutcome, Visibility } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { useConnectRemote } from './mutations';
	import { useOwners } from './queries';
	import RemoteTargetFields from './RemoteTargetFields.svelte';

	interface Props {
		open: boolean;
		/** 接続するプロジェクト。null のときは何もしない */
		projectId: string | null;
		projectName: string;
		onclose: () => void;
		/** 接続できた（アップロードの成否にかかわらず）。画面は状態を取り直す */
		onconnected?: (outcome: RemoteOutcome) => void;
	}

	let { open, projectId, projectName, onclose, onconnected }: Props = $props();

	const owners = useOwners();
	const ownerList = $derived(owners.data ?? []);
	const defaultOwner = $derived(ownerList.find((o) => o.canCreate)?.id ?? '');
	// 保存先の一覧を取得できない（未ログインなど）ときは、接続できない
	const loggedOut = $derived(owners.isError);

	let ownerId = $state('');
	let visibility = $state<Visibility>('private');
	let publicConfirmed = $state(false);
	let name = $state('');
	let submitted = $state(false);

	const connect = useConnectRemote((outcome) => {
		onconnected?.(outcome);
		// 接続できていれば閉じる。作成前に失敗したときは入力を残し、直して再実行できるようにする
		onclose();
	});

	$effect(() => {
		if (!open) return;
		submitted = false;
		visibility = 'private';
		publicConfirmed = false;
		name = '';
	});

	// 保存先の一覧が届いたら、作成できる最初の保存先を選んでおく
	$effect(() => {
		if (open && !ownerList.some((o) => o.id === ownerId && o.canCreate)) ownerId = defaultOwner;
	});

	const ownerError = $derived(
		submitted && !ownerList.some((o) => o.id === ownerId && o.canCreate)
			? t('add_project.error_owner')
			: undefined
	);
	const confirmError = $derived(
		submitted && visibility === 'public' && !publicConfirmed
			? t('add_project.error_public_confirm')
			: undefined
	);

	function submit() {
		submitted = true;
		if (!projectId || ownerError || confirmError) return;
		connect.mutate({
			id: projectId,
			name: projectName,
			input: {
				ownerId,
				visibility,
				publicConfirmed: visibility === 'public' && publicConfirmed,
				name: name.trim()
			}
		});
	}
</script>

<Dialog
	{open}
	variant="form"
	title={t('connect.title', { name: projectName })}
	description={t('connect.description')}
	busy={connect.isPending}
	{onclose}
>
	{#if loggedOut}
		<p class="m:0 type-body" role="alert">{t('connect.need_login')}</p>
	{:else}
		<form
			id="connect-remote-form"
			class="flex flex-direction:column gap:4"
			onsubmit={(event) => {
				event.preventDefault();
				submit();
			}}
		>
			<RemoteTargetFields
				owners={ownerList}
				loading={owners.isPending}
				bind:ownerId
				bind:visibility
				bind:publicConfirmed
				{ownerError}
				{confirmError}
			/>
			<TextField
				bind:value={name}
				label={t('connect.name')}
				description={t('connect.name_hint')}
				placeholder={t('connect.name_placeholder')}
				maxLength={100}
			/>
		</form>
	{/if}

	{#snippet actions()}
		<span class="flex:1"></span>
		<Button variant="secondary" disabled={connect.isPending} onclick={onclose}>
			{t('add_project.cancel')}
		</Button>
		{#if !loggedOut}
			<Button loading={connect.isPending} onclick={submit}>{t('header.connect')}</Button>
		{/if}
	{/snippet}
</Dialog>
