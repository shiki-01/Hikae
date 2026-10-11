<script lang="ts">
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { interruptedOperationName } from './interrupted';

	interface Props {
		open: boolean;
		/** 途中で止まった操作の名前。不明なら null */
		operation: string | null;
		/** 操作の前の状態へ戻している間は true */
		recovering?: boolean;
		/** 「このまま続ける」を実行している間は true */
		dismissing?: boolean;
		onrecover: () => void;
		ondismiss: () => void;
		onclose: () => void;
	}

	let {
		open,
		operation,
		recovering = false,
		dismissing = false,
		onrecover,
		ondismiss,
		onclose
	}: Props = $props();
	const busy = $derived(recovering || dismissing);
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
		<Button loading={recovering} disabled={busy} onclick={onrecover}>
			{t('interrupted.recover')}
		</Button>
	{/snippet}
</Dialog>
