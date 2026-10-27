import './styles/fonts.css';
import './styles/tokens.css';
import './styles/accents.css';
import './styles/base.css';
import './styles/motion.css';
import './styles/mobile.css';
import { mount } from 'svelte';
import App from './App.svelte';
import UnlockCompact from './screens/UnlockCompact.svelte';
import { compactSpec } from './lib/browserLink';
import { trackModality } from './lib/modality';
import { installMobile } from './lib/mobile';

trackModality();
installMobile();

const compact = compactSpec();
const app = compact ? mount(UnlockCompact, { target: document.getElementById('app')!, props: { spec: compact } }) : mount(App, { target: document.getElementById('app')! });

export default app;
