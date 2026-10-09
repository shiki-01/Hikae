<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';

	interface Props {
		/** 一覧から外すプロジェクトの名前。null のときは閉じている */
		name: string | null;
		/** 外している間は true */
		pending?: boolean;
		onconfirm: () => void;
		oncancel: () => void;
	}

	let { name, pending = false, onconfirm, oncancel }: Props = $props();
</script>

<!-- 取り消せる操作（同じフォルダを追加し直せる）なので、危険を示す色は使わない -->
<Dialog
	open={name !== null}
	variant="confirm"
	title={t('remove_project.title', { name: name ?? '' })}
	busy={pending}
	onclose={oncancel}
>
	<div class="flex flex-direction:column gap:2">
		<p class="m:0 type-body">{t('remove_project.effect')}</p>
		<p class="m:0 type-body fg:fg-muted">{t('remove_project.again')}</p>
	</div>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('remove_project.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" disabled={pending} onclick={oncancel}>
			{t('remove_project.cancel')}
		</Button>
		<Button variant="primary" loading={pending} disabled={pending} onclick={onconfirm}>
			{t('remove_project.confirm')}
		</Button>
	{/snippet}
</Dialog>
