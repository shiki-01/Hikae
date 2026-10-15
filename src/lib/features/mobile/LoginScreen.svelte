<script lang="ts">
	import { onDestroy } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import { mobileApi } from '#lib/api/mobile-index.js';
	import type { DeviceFlow } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import {
		failureForOutcome,
		formatCountdown,
		isLoginSetupError,
		setupFailure,
		type LoginFailureView
	} from '#lib/features/projects/login-flow.js';

	interface Props {
		onlogged: () => void;
	}

	let { onlogged }: Props = $props();

	type Phase = 'idle' | 'starting' | 'waiting';

	let phase = $state<Phase>('idle');
	let flow = $state<DeviceFlow | null>(null);
	let failure = $state<LoginFailureView | null>(null);
	let remaining = $state(0);
	/** 始めた試行の番号。古い試行の結果や、画面を離れたあとの結果は使わない */
	let attempt = 0;

	async function start(): Promise<void> {
		const current = ++attempt;
		phase = 'starting';
		failure = null;
		try {
			const next = await mobileApi.startLogin();
			if (current !== attempt) return;
			flow = next;
			remaining = next.expiresInSecs;
			phase = 'waiting';
			const outcome = await mobileApi.waitLogin();
			if (current !== attempt) return;
			flow = null;
			phase = 'idle';
			if (outcome === 'succeeded') {
				onlogged();
				return;
			}
			failure = failureForOutcome(outcome);
		} catch (error) {
			if (current !== attempt) return;
			flow = null;
			phase = 'idle';
			if (isLoginSetupError(error)) failure = setupFailure();
			else reportError(error, { retry: () => void start() });
		}
	}

	function cancel(): void {
		attempt += 1;
		flow = null;
		phase = 'idle';
		void mobileApi.cancelLogin();
	}

	async function openPage(): Promise<void> {
		try {
			await mobileApi.openLoginPage();
		} catch (error) {
			reportError(error);
		}
	}

	// 確認コードの残り時間を 1 秒ごとに減らす
	$effect(() => {
		if (phase !== 'waiting') return;
		const timer = setInterval(() => {
			remaining = Math.max(0, remaining - 1);
		}, 1000);
		return () => clearInterval(timer);
	});

	onDestroy(() => {
		attempt += 1;
	});
</script>

<section class="flex flex-direction:column gap:4 p:6 flex:1 min-h:0 overflow:auto">
	<h1 class="m:0 type-title">{t('mobile.login.title')}</h1>
	<p class="m:0 type-body fg:fg-muted">{t('mobile.login.description')}</p>

	{#if failure}
		<div role="alert" class="flex flex-direction:column gap:2 p:4 r:lg b:1px|solid|state-danger">
			<p class="m:0 type-heading">{t(failure.title)}</p>
			<p class="m:0 type-body fg:fg-muted">{t(failure.dataIsSafe)}</p>
			<p class="m:0 type-body">{t(failure.nextAction)}</p>
			{#if failure.retryable}
				<div>
					<Button class="min-h:44px" onclick={() => void start()}>{t('login.retry')}</Button>
				</div>
			{/if}
		</div>
	{/if}

	{#if phase === 'idle'}
		<div>
			<Button class="min-h:44px" onclick={() => void start()}>{t('mobile.login.button')}</Button>
		</div>
	{:else if phase === 'starting'}
		<div class="flex align-items:center gap:2">
			<Spinner />
			<span class="type-body fg:fg-muted">{t('mobile.login.starting')}</span>
		</div>
	{:else if flow}
		<div class="flex flex-direction:column gap:2 p:4 r:lg bg:bg-subtle b:1px|solid|border">
			<span class="type-small fg:fg-muted">{t('mobile.login.code_label')}</span>
			<span class="type-title font-weight:700">{flow.userCode}</span>
			<p class="m:0 type-body">{t('mobile.login.instruction')}</p>
			<p class="m:0 type-small fg:fg-muted">
				{t('login.expires', { time: formatCountdown(remaining) })}
			</p>
		</div>
		<div class="flex flex-direction:column gap:2">
			<Button class="min-h:44px" onclick={() => void openPage()}>{t('mobile.login.open')}</Button>
			<div class="flex align-items:center gap:2">
				<Spinner size="sm" />
				<span class="type-small fg:fg-muted">{t('mobile.login.waiting')}</span>
			</div>
			<Button variant="ghost" class="min-h:44px" onclick={cancel}>{t('login.cancel')}</Button>
		</div>
	{/if}
</section>
