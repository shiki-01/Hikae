<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import {
		AlertTriangle,
		Check,
		CheckCircle2,
		CloudDownload,
		Copy,
		ExternalLink,
		FolderOpen,
		FolderPlus
	} from '@lucide/svelte';
	import { onDestroy, type Component } from 'svelte';
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import { api } from '#lib/api/index.js';
	import { keys } from '#lib/api/keys.js';
	import type { AddProjectMode, DeviceFlow, Project } from '#lib/api/types.js';
	import Badge from '#lib/components/Badge.svelte';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { reportError } from '#lib/features/notifications/store.svelte.js';
	import AddProjectDialog from './AddProjectDialog.svelte';
	import {
		failureForOutcome,
		formatCountdown,
		isHttpsUrl,
		isLoginSetupError,
		setupFailure,
		type LoginFailureView
	} from './login-flow';
	import { useCompleteOnboarding } from './mutations';
	import { useSession } from './queries';

	const client = useQueryClient();
	const session = useSession();

	let step = $state<1 | 2 | 3>(1);
	let phase = $state<'idle' | 'starting' | 'waiting' | 'done'>('idle');
	let flow = $state<DeviceFlow | null>(null);
	let failure = $state<LoginFailureView | null>(null);
	let remaining = $state(0);
	let copied = $state(false);
	let mode = $state<AddProjectMode | null>(null);
	let added = $state<Project | null>(null);

	// すでにログイン済み（アプリを開き直した場合など）なら、ログインの手順は飛ばす
	const loggedIn = $derived(
		phase === 'done' || (session.data?.loggedIn === true && !session.data.reauthRequired)
	);

	$effect(() => {
		if (phase !== 'waiting' || !flow) return;
		const end = Date.now() + flow.expiresInSecs * 1000;
		remaining = flow.expiresInSecs;
		const timer = setInterval(() => {
			remaining = Math.max(0, Math.ceil((end - Date.now()) / 1000));
		}, 1000);
		return () => clearInterval(timer);
	});

	onDestroy(() => {
		// 待機中に画面を離れたら、バックエンドの待機も止める
		if (phase === 'waiting') void api.cancelLogin().catch(() => {});
	});

	async function beginLogin() {
		if (phase === 'starting' || phase === 'waiting') return;
		failure = null;
		phase = 'starting';
		try {
			flow = await api.startLogin();
		} catch (error) {
			phase = 'idle';
			if (isLoginSetupError(error)) failure = setupFailure();
			else reportError(error);
			return;
		}
		phase = 'waiting';
		try {
			const outcome = await api.waitLogin();
			if (outcome === 'succeeded') {
				phase = 'done';
				await client.invalidateQueries({ queryKey: keys.session });
			} else {
				phase = 'idle';
				failure = failureForOutcome(outcome);
			}
		} catch (error) {
			phase = 'idle';
			reportError(error);
		} finally {
			flow = null;
		}
	}

	async function cancelLogin() {
		try {
			await api.cancelLogin();
		} catch (error) {
			reportError(error);
		}
	}

	const complete = useCompleteOnboarding();

	const steps: MessageKey[] = ['wizard.step1', 'wizard.step2', 'wizard.step3'];
	const cards: { mode: AddProjectMode; icon: Component; title: MessageKey; text: MessageKey }[] = [
		{
			mode: 'existing',
			icon: FolderOpen,
			title: 'add_project.existing',
			text: 'add_project.existing_text'
		},
		{
			mode: 'github',
			icon: CloudDownload,
			title: 'add_project.from_github',
			text: 'add_project.from_github_text'
		},
		{ mode: 'new', icon: FolderPlus, title: 'add_project.new', text: 'add_project.new_text' }
	];

	const canNext = $derived(step === 1 ? loggedIn : step === 2 ? added !== null : true);

	async function copyCode() {
		try {
			await navigator.clipboard.writeText(flow?.userCode ?? '');
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}

	async function next() {
		if (step === 1) step = 2;
		else if (step === 2) step = 3;
		else {
			await complete.mutateAsync();
			await goto(
				added ? `${resolve('/project')}?id=${encodeURIComponent(added.id)}` : resolve('/')
			);
		}
	}

	function back() {
		if (step === 2) step = 1;
		else if (step === 3) step = 2;
	}
</script>

<div class="min-h:100vh flex flex-direction:column align-items:center bg:bg fg:fg px:4 py:8">
	<header class="flex flex-direction:column align-items:center gap:4 mb:6">
		<h1 class="m:0 type-title">{t('wizard.title')}</h1>
		<ol class="m:0 p:0 list-style:none flex align-items:center gap:4">
			{#each steps as label, index (label)}
				{@const number = index + 1}
				{@const done = number < step}
				<li
					aria-current={number === step ? 'step' : undefined}
					class="flex align-items:center gap:2 type-body"
				>
					<span
						class={`size:28px r:full flex align-items:center justify-content:center type-small font-weight:700 ${
							done
								? 'bg:accent-subtle fg:fg b:1px|solid|state-saved'
								: number === step
									? 'bg:accent fg:accent-fg'
									: 'bg:bg-subtle fg:fg-muted b:1px|solid|border-strong'
						}`}
					>
						{#if done}
							<Check size={14} aria-hidden="true" />
							<span class="sr-only">{t('wizard.step_done')}</span>
						{:else}
							{number}
						{/if}
					</span>
					<span class={number === step ? 'font-weight:700' : 'fg:fg-muted'}>{t(label)}</span>
				</li>
			{/each}
		</ol>
	</header>

	<main class="w:100% max-w:480px flex flex-direction:column gap:6">
		<section class="p:6 r:lg bg:bg-raised b:1px|solid|border flex flex-direction:column gap:4">
			{#if step === 1}
				<h2 class="m:0 type-heading">{t('wizard.step1')}</h2>
				<p class="m:0 type-body fg:fg-muted">{t('wizard.login.description')}</p>

				{#if loggedIn}
					<p class="m:0 flex align-items:center gap:2 type-body" role="status">
						<CheckCircle2 size={18} class="fg:state-saved" aria-hidden="true" />
						{session.data?.userLogin
							? t('login.logged_in_as', { name: session.data.userLogin })
							: t('wizard.login.done')}
					</p>
				{:else}
					{#if failure}
						<div
							class="flex flex-direction:column gap:2 p:4 r:md bg:bg-subtle b:1px|solid|state-danger"
							role="alert"
							data-testid="login-failure"
						>
							<div class="flex align-items:start gap:2">
								<AlertTriangle
									size={18}
									class="fg:state-danger flex-shrink:0 mt:2px"
									aria-hidden="true"
								/>
								<p class="m:0 type-body font-weight:700">{t(failure.title)}</p>
							</div>
							{#if failure.kind === 'setup'}
								<Badge
									variant="danger"
									label={t('login.setup_badge')}
									class="align-self:flex-start"
								/>
							{/if}
							<p class="m:0 type-body">{t(failure.dataIsSafe)}</p>
							<p class="m:0 type-body fg:fg-muted">{t(failure.nextAction)}</p>
						</div>
					{:else if session.data?.reauthRequired && phase === 'idle'}
						<div
							class="flex flex-direction:column gap:1 p:4 r:md bg:bg-subtle b:1px|solid|state-unsaved"
							role="alert"
						>
							<p class="m:0 type-body font-weight:700">{t('error.E01.title')}</p>
							<p class="m:0 type-body fg:fg-muted">{t('error.E01.message')}</p>
						</div>
					{/if}

					{#if phase === 'waiting' && flow}
						<div class="flex flex-direction:column align-items:center gap:2 p:4 r:md bg:bg-subtle">
							<span class="type-small fg:fg-muted">{t('wizard.login.code_label')}</span>
							<div class="flex align-items:center gap:3">
								<span
									class="type-title font-family:mono letter-spacing:.15em"
									data-testid="user-code"
								>
									{flow.userCode}
								</span>
								<Button size="sm" variant="secondary" onclick={copyCode}>
									{#if copied}
										<Check size={14} aria-hidden="true" />
										{t('wizard.login.copied')}
									{:else}
										<Copy size={14} aria-hidden="true" />
										{t('wizard.login.copy')}
									{/if}
								</Button>
							</div>
							<span class="type-small fg:fg-muted text-align:center"
								>{t('wizard.login.instruction')}</span
							>
							{#if isHttpsUrl(flow.verificationUri)}
								<a
									href={flow.verificationUri}
									target="_blank"
									rel="noopener noreferrer"
									class="inline-flex align-items:center gap:2 px:3 py:1 r:md bg:bg-raised fg:fg b:1px|solid|border-strong type-small font-weight:500 text-decoration:none"
								>
									<ExternalLink size={14} aria-hidden="true" />
									{t('login.open_browser')}
								</a>
								<span class="type-small fg:fg-muted text-align:center overflow-wrap:anywhere">
									{t('login.url_hint')}<br />
									<span class="font-family:mono">{flow.verificationUri}</span>
								</span>
							{/if}
						</div>
						<div class="flex align-items:center justify-content:space-between gap:3">
							<p class="m:0 flex align-items:center gap:2 type-body" role="status">
								<Spinner size="sm" />
								{t('wizard.login.waiting')}
							</p>
							<Button variant="ghost" size="sm" onclick={cancelLogin}>{t('login.cancel')}</Button>
						</div>
						<p class="m:0 type-small fg:fg-muted">
							{t('login.expires', { time: formatCountdown(remaining) })}
						</p>
					{:else if !failure || failure.retryable}
						<Button loading={phase === 'starting'} onclick={beginLogin}>
							{failure ? t('login.retry') : t('wizard.login.button')}
						</Button>
					{/if}
				{/if}
			{:else if step === 2}
				<h2 class="m:0 type-heading">{t('wizard.step2')}</h2>
				<p class="m:0 type-body fg:fg-muted">{t('wizard.project_description')}</p>
				{#if added}
					<p class="m:0 flex align-items:center gap:2 type-body" role="status">
						<CheckCircle2 size={18} class="fg:state-saved" aria-hidden="true" />
						{t('wizard.project_added', { name: added.name })}
					</p>
				{:else}
					<div class="flex flex-direction:column gap:3">
						{#each cards as card (card.mode)}
							{@const Icon = card.icon}
							<button
								type="button"
								onclick={() => (mode = card.mode)}
								class="flex align-items:center gap:4 p:4 r:md bg:bg-raised fg:fg b:1px|solid|border-strong b:1px|solid|accent:hover text-align:left cursor:pointer"
							>
								<span
									class="size:40px r:md bg:accent-subtle flex align-items:center justify-content:center flex-shrink:0"
									aria-hidden="true"
								>
									<Icon size={20} class="fg:accent" />
								</span>
								<span class="flex flex-direction:column">
									<span class="type-body font-weight:700">{t(card.title)}</span>
									<span class="type-small fg:fg-muted">{t(card.text)}</span>
								</span>
							</button>
						{/each}
					</div>
				{/if}
			{:else}
				<h2 class="m:0 type-heading">{t('wizard.step3')}</h2>
				<p class="m:0 type-body">{t('wizard.done_message')}</p>
				<p class="m:0 type-body fg:fg-muted">{t('wizard.done_detail')}</p>
			{/if}
		</section>

		<div class="flex justify-content:space-between">
			<Button variant="secondary" disabled={step === 1} onclick={back}>{t('wizard.back')}</Button>
			<Button disabled={!canNext} loading={complete.isPending} onclick={next}>
				{step === 3 ? t('wizard.finish') : t('wizard.next')}
			</Button>
		</div>
	</main>
</div>

<AddProjectDialog
	open={mode !== null}
	initialMode={mode}
	onclose={() => (mode = null)}
	onadded={(project) => (added = project)}
/>
