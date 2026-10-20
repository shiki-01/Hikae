<script lang="ts">
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { interruptedOperationName, isResumableOperation } from './interrupted';

	interface Props {
		open: boolean;
		/** 途中で止まった操作の名前。不明なら null */
		operation: string | null;
		/** 操作の前の状態へ戻している間は true */
		recovering?: boolean;
		/** 「続きを行う」を実行している間は true（復旧と、その後のやり直しの間） */
		resuming?: boolean;
		/** 「このまま続ける」を実行している間は true */
		dismissing?: boolean;
		onrecover: () => void;
		onresume: () => void;
		ondismiss: () => void;
		onclose: () => void;
	}

	let {
		open,
		operation,
		recovering = false,
		resuming = false,
		dismissing = false,
		onrecover,
		onresume,
		ondismiss,
		onclose
	}: Props = $props();
	const busy = $derived(recovering || resuming || dismissing);
	// 「続きを行う」は取り込み・アップロードだけ出す（保存は保存メモが失われているため出さない。設計書 6.3）
	const resumable = $derived(isResumableOperation(operation));
</script>

<Dialog
	{open}
	variant="warning"
	title={t('interrupted.title')}
	description={t('interrupted.description', { name: interruptedOperationName(operation) })}
	{busy}
	{onclose}
>
	<div class="flex flex-direction:column gap:2">
		<p class="m:0 type-body">{t('interrupted.next')}</p>
		{#if resumable}
			<p class="m:0 type-small fg:fg-muted">{t('interrupted.resume_hint')}</p>
		{/if}
		<p class="m:0 type-small fg:fg-muted">{t('interrupted.recover_hint')}</p>
		<p class="m:0 type-small fg:fg-muted">{t('interrupted.dismiss_hint')}</p>
	</div>

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('interrupted.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" disabled={busy} onclick={onclose}>
			{t('interrupted.close')}
		</Button>
		<Button variant="secondary" loading={dismissing} disabled={busy} onclick={ondismiss}>
			{t('interrupted.dismiss')}
		</Button>
		{#if resumable}
			<Button variant="secondary" loading={recovering} disabled={busy} onclick={onrecover}>
				{t('interrupted.recover')}
			</Button>
			<Button loading={resuming} disabled={busy} onclick={onresume}>
				{t('interrupted.resume')}
			</Button>
		{:else}
			<Button loading={recovering} disabled={busy} onclick={onrecover}>
				{t('interrupted.recover')}
			</Button>
		{/if}
	{/snippet}
</Dialog>
