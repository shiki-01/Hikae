<script lang="ts">
	import { AlertTriangle } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { Owner, Visibility } from '#lib/api/types.js';
	import Checkbox from '#lib/components/Checkbox.svelte';
	import OwnerPicker from '#lib/components/OwnerPicker.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';

	interface Props {
		owners: Owner[];
		loading?: boolean;
		ownerId: string;
		visibility: Visibility;
		/** 公開にすることの警告を確認した */
		publicConfirmed: boolean;
		ownerError?: string;
		confirmError?: string;
	}

	let {
		owners,
		loading = false,
		ownerId = $bindable(),
		visibility = $bindable(),
		publicConfirmed = $bindable(),
		ownerError,
		confirmError
	}: Props = $props();

	// 非公開に戻したら、公開の確認は取り消す（公開に切り替えるたびに確認し直す）
	$effect(() => {
		if (visibility === 'private') publicConfirmed = false;
	});
</script>

<!-- 保存先（個人 / チーム）と公開範囲。プロジェクトの追加と、後からの GitHub への接続で共有する -->
<div class="flex flex-direction:column gap:2">
	<span class="type-small font-weight:500" id="owner-label">{t('add_project.owner')}</span>
	{#if loading}
		<Skeleton class="h:48px" />
	{:else}
		<OwnerPicker {owners} bind:value={ownerId} ariaLabel={t('add_project.owner')} />
	{/if}
	{#if ownerError}
		<p class="m:0 type-small fg:state-danger" role="alert">{ownerError}</p>
	{/if}
</div>

<div class="flex flex-direction:column gap:2">
	<span class="type-small font-weight:500">{t('add_project.visibility')}</span>
	<SegmentedControl
		bind:value={visibility}
		ariaLabel={t('add_project.visibility')}
		options={[
			{ value: 'private', label: t('add_project.visibility_private') },
			{ value: 'public', label: t('add_project.visibility_public') }
		]}
		class="align-self:flex-start"
	/>
	{#if visibility === 'public'}
		<p
			class="m:0 flex align-items:start gap:2 p:3 r:md bg:bg-subtle b:1px|solid|state-unsaved type-body"
			role="note"
		>
			<AlertTriangle size={18} class="fg:state-unsaved flex-shrink:0 mt:2px" aria-hidden="true" />
			{t('add_project.visibility_warning')}
		</p>
		<Checkbox bind:checked={publicConfirmed} label={t('add_project.visibility_confirm')} />
		{#if confirmError}
			<p class="m:0 type-small fg:state-danger" role="alert">{confirmError}</p>
		{/if}
	{:else}
		<p class="m:0 type-small fg:fg-muted">{t('add_project.visibility_private_hint')}</p>
	{/if}
</div>
