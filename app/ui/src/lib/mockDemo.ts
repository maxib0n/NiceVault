// English sample content for the in-browser mock (`?demo=1`): the volumes, files, tags and
// password-manager entries the README screenshots and GIFs are made from (e2e/shots/).
// The normal suite keeps the Italian seed.

import type { PmEntryInput } from './passwords';

type Seed = [name: string, sizeOrChildren: number | Seed[], asset?: string];

const MB = 1024 * 1024;
const photo = (n: number) => `/mock/photo-${String(n).padStart(2, '0')}.jpg`;

/** The stick NiceVault "runs from": the data partition of a NiceVault device. */
export const DEMO_STICK = '/run/media/alex/VaultData';
export const DEMO_STICK_LABEL = 'VaultData';
export const DEMO_HOME = '/home/alex/Documents';

export const DEMO_DOCUMENTS: Seed[] = [
  [
    'Photos',
    [
      [
        'Alps 2025',
        [
          ['Alpine lake.jpg', 4.6 * MB, photo(1)],
          ['Highlands.jpg', 3.8 * MB, photo(2)],
          ['Dusk over the lake.jpg', 3.1 * MB, photo(3)],
          ['Lake reflection.jpg', 5.2 * MB, photo(4)],
          ['Forest trail.jpg', 4.4 * MB, photo(5)],
        ],
      ],
      [
        'New York',
        [
          ['Carnegie Deli.jpg', 3.7 * MB, photo(7)],
          ['Portrait, ivy wall.jpg', 2.9 * MB, photo(6)],
          ['Laughing.jpg', 2.4 * MB, photo(8)],
        ],
      ],
      [
        'Summer',
        [
          ['Desert walk.jpg', 3.3 * MB, photo(9)],
          ['Pier at noon.jpg', 4.1 * MB, photo(10)],
          ['Beach from above.jpg', 5.0 * MB, photo(11)],
          ['Coneflowers.jpg', 3.9 * MB, photo(14)],
        ],
      ],
      [
        'Mochi',
        [
          ['Mochi on the shelf.jpg', 2.2 * MB, photo(12)],
          ['Mochi, close-up.jpg', 1.8 * MB, photo(13)],
        ],
      ],
    ],
  ],
  [
    'Work',
    [
      ['Archive 2023', []],
      ['Lease agreement.pdf', 412_000],
      ['Invoice 2025-031.pdf', 88_000],
      ['Kitchen quote.pdf', 1.4 * MB],
      ['Budget 2026.xlsx', 61_000],
      ['Meeting notes.md', 1_200],
      ['Q3 results.pptx', 7.8 * MB],
    ],
  ],
  [
    'Personal',
    [
      [
        'Recipes',
        [
          ['Ragu.txt', 420, '/mock/recipe.txt'],
          ['Tiramisu.txt', 380, '/mock/recipe.txt'],
          ['Focaccia.txt', 510, '/mock/recipe.txt'],
        ],
      ],
      ['Passport scan.jpg', 1.1 * MB, '/mock/scan.jpg'],
      ['Health insurance card.png', 640_000, '/mock/card.png'],
      ['Blood test 2025.pdf', 230_000],
    ],
  ],
  [
    'Video',
    [
      ['Alps timelapse.mp4', 86 * MB, '/mock/demo-clip.webm'],
      ['Birthday.webm', 148 * MB, '/mock/demo-clip.webm'],
      ['Drone, coast.mov', 412 * MB, '/mock/demo-clip.webm'],
    ],
  ],
  [
    'Audio',
    [
      ['Voice memo 01.ogg', 410_000],
      ['Voice memo 02.ogg', 1.2 * MB],
      ['Message from Dad.m4a', 2.4 * MB],
    ],
  ],
  [
    'Keys',
    [
      ['id_ed25519.pub', 98],
      ['id_ed25519', 411, '/mock/key.pub'],
    ],
  ],
  ['README.txt', 2_100],
  ['Archive 2019.zip', 312 * MB],
  ['Phone backup.tar.gz', 1.7 * 1024 * MB],
  ['Cover.jpg', 1.3 * MB, photo(14)],
];

export const DEMO_TAX: Seed[] = [
  ['Receipts', [['Pharmacy, March.pdf', 120_000], ['Dentist.pdf', 98_000], ['Optician.pdf', 77_000]]],
  ['Tax return 2023.pdf', 940_000],
  ['Tax return 2024.pdf', 1.02 * MB],
  ['Withholding statement 2024.pdf', 310_000],
];

export const DEMO_JOURNAL: Seed[] = [
  ['2025', [['January.md', 4_200, '/mock/notes.md'], ['February.md', 3_100, '/mock/notes.md']]],
  ['Letters', [['To Sam.txt', 1_900, '/mock/readme.txt']]],
  ['Thoughts.md', 2_600, '/mock/notes.md'],
];

export const DEMO_NAMES = { documents: 'Documents', tax: 'Tax Archive', journal: 'Journal', old: 'Old laptop' };
export const DEMO_SLOTS = { main: 'Main', spare: 'Spare, in the safe', duress: 'Duress' };

/** [name, colour, names of the nodes carrying it] */
export const DEMO_TAGS: [string, string, string[]][] = [
  ['Important', 'velluto', ['Lease agreement.pdf', 'Passport scan.jpg', 'Keys']],
  ['Travel', 'ottanio', ['Alps 2025', 'Alpine lake.jpg', 'Lake reflection.jpg', 'Pier at noon.jpg']],
  ['Family', 'bottiglia', ['Mochi', 'Message from Dad.m4a', 'Laughing.jpg']],
  ['To review', 'seppia', ['Invoice 2025-031.pdf', 'Kitchen quote.pdf']],
  ['Work', 'prussia', ['Budget 2026.xlsx', 'Q3 results.pptx', 'Meeting notes.md']],
  ['New', 'neutral', []],
];

const DAY = 86_400_000;

export function demoPmSeeds(now: number): { input: PmEntryInput; history?: { password: string; changed: number }[]; age: number }[] {
  return [
    {
      age: 120,
      input: {
        title: 'Fastmail', username: 'alex@example.com', password: 'Heron-Lantern-Quiet-81', urls: ['https://app.fastmail.com'],
        totp: 'otpauth://totp/Fastmail:alex@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Fastmail', folder: 'Personal', tags: ['email'], favorite: true,
        notes: 'Recovery: the number on the fridge.',
      },
      history: [{ password: 'old-mail-2023', changed: now - 200 * DAY }],
    },
    {
      age: 60,
      input: {
        title: 'Harbor Credit Union', username: '00412789', password: 'x7#Lq9!vR2@pZ4', urls: ['https://online.harborcu.example.com'],
        totp: 'otpauth://totp/Harbor%20Credit%20Union:00412789?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Harbor%20Credit%20Union&algorithm=SHA256&digits=8&period=60',
        folder: 'Personal/Banking', tags: ['bank', 'important'], favorite: true,
        fields: [{ name: 'Member number', value: '00412789', hidden: false }, { name: 'Device PIN', value: '48213', hidden: true }],
      },
    },
    { age: 300, input: { title: 'Home Wi-Fi', password: 'cherry-mill-lighthouse-12', folder: 'Home', notes: 'Network: Alder-5G' } },
    {
      age: 15,
      input: {
        title: 'Work VPN and Git', username: 'alex', password: 'S3rv3r!Work#2026', urls: ['https://vpn.example.com', 'https://git.example.com'],
        totp: 'JBSWY3DPEHPK3PXP', folder: 'Work', tags: ['work'], fields: [{ name: 'SSH key passphrase', value: 'swallow footpath sail', hidden: true }],
      },
    },
    { age: 30, input: { title: 'Streaming', username: 'family@example.com', password: 'Popcorn&Sofa99', urls: ['https://www.streaming.example.com'], tags: ['shared'] } },
    { age: 5, input: { title: 'Tax portal', username: 'alex@example.com', password: 'Digital-Identity-55!', urls: ['https://login.taxes.example.gov'], folder: 'Personal', tags: ['important'] } },
  ];
}
