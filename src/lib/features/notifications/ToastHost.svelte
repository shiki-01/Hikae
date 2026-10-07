<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Toast from '#lib/components/Toast.svelte';
	import { dismissToast, notifications } from './store.svelte';

	let host = $state<HTMLDivElement>();

	$effect(() => {
		const el = host;
		if (!el) return;
		const count = notifications.toasts.length;
		try {
			if (el.matches(':popover-open')) el.hidePopover();
			if (count > 0) el.showPopover();
		} catch {
			return;
		}
	});
</script>

<div
	bind:this={host}
	popover="manual"
	aria-label={t('toast.region')}
	class="fixed top:auto left:auto right:4 bottom:4 m:0 p:0 b:0 bg:transparent overflow:visible w:400px max-w:90vw"
>
	<div class="flex flex-direction:column gap:2">
		{#each notifications.toasts as toast (toast.id)}
			<Toast
				type={toast.type}
				message={toast.message}
				actionLabel={toast.actionLabel}
				onaction={() => {
					dismissToast(toast.id);
					toast.onaction?.();
				}}
				onclose={() => dismissToast(toast.id)}
			/>
		{/each}
	</div>
</div>
