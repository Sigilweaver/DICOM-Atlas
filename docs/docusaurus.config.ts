import { themes as prismThemes } from 'prism-react-renderer';
import type { Config } from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
    title: 'DICOM-Atlas',
    tagline: 'An open registry of public + private DICOM tags, queryable from Rust, C, and Python',
    favicon: 'img/favicon.ico',

    markdown: {
        mermaid: true,
        hooks: {
            onBrokenMarkdownLinks: 'warn',
        },
    },
    plugins: ['docusaurus-plugin-llms-txt'],
    themes: ['@docusaurus/theme-mermaid'],

    url: 'https://sigilweaver.app',
    baseUrl: '/dicom-atlas/docs/',

    organizationName: 'Sigilweaver',
    projectName: 'DICOM-Atlas',

    onBrokenLinks: 'throw',

    i18n: {
        defaultLocale: 'en',
        locales: ['en'],
    },

    presets: [
        [
            'classic',
            {
                docs: {
                    routeBasePath: '/',
                    sidebarPath: './sidebars.ts',
                    editUrl: 'https://github.com/Sigilweaver/DICOM-Atlas/tree/main/docs/',
                },
                blog: false,
                sitemap: {
                    changefreq: 'weekly',
                    priority: 0.5,
                    filename: 'sitemap.xml',
                },
                theme: {
                    customCss: './src/css/custom.css',
                },
            } satisfies Preset.Options,
        ],
    ],

    themeConfig: {
        metadata: [
            { name: 'keywords', content: 'DICOM, medical imaging, private tags, vendor conformance, rkyv, Rust, Python' },
            { name: 'description', content: 'DICOM-Atlas is an open registry of public + private DICOM tags compiled from vendor conformance statements and the PS3.6 standard. Queryable from Rust, C, or Python in O(log n).' },
        ],
        colorMode: {
            defaultMode: 'dark',
            disableSwitch: false,
            respectPrefersColorScheme: true,
        },
        navbar: {
            title: 'Sigilweaver',
            logo: {
                alt: 'Sigilweaver logo',
                src: 'img/logo.svg',
                href: 'https://sigilweaver.app',
                target: '_self',
            },
            items: [
                {
                    type: 'dropdown',
                    label: 'Projects',
                    position: 'left',
                    items: [
                        { label: 'DICOM-Atlas', href: 'https://sigilweaver.app/dicom-atlas/docs/' },
                        { label: 'OpenKSpace', href: 'https://sigilweaver.app/openkspace/docs/' },
                        { label: 'BioLance', href: 'https://sigilweaver.app/biolance/docs/' },
                        { label: 'OpenProteo', href: 'https://sigilweaver.app/openproteo/docs/' },
                        { label: 'All projects', href: 'https://sigilweaver.app/docs/' },
                    ],
                },
                {
                    href: 'https://github.com/Sigilweaver/DICOM-Atlas',
                    label: 'GitHub',
                    position: 'right',
                },
            ],
        },
        footer: {
            style: 'dark',
            links: [
                {
                    title: 'Project',
                    items: [
                        { label: 'GitHub', href: 'https://github.com/Sigilweaver/DICOM-Atlas' },
                        { label: 'Issues', href: 'https://github.com/Sigilweaver/DICOM-Atlas/issues' },
                        { label: 'crates.io', href: 'https://crates.io/crates/dicom-map' },
                        { label: 'PyPI', href: 'https://pypi.org/project/dicom-map/' },
                    ],
                },
                {
                    title: 'Related',
                    items: [
                        { label: 'OpenKSpace', href: 'https://sigilweaver.app/openkspace/docs/' },
                        { label: 'BioLance', href: 'https://sigilweaver.app/biolance/docs/' },
                        { label: 'All projects', href: 'https://sigilweaver.app/docs/' },
                    ],
                },
                {
                    title: 'Legal',
                    items: [
                        { label: 'Terms of Use', href: 'https://sigilweaver.app/terms' },
                        { label: 'Privacy Policy', href: 'https://sigilweaver.app/privacy' },
                    ],
                },
            ],
            copyright: `Copyright ${new Date().getFullYear()} Sigilweaver Holdings LLC. DICOM-Atlas code is Apache-2.0 licensed; the tag data is licensed under <a href="https://creativecommons.org/licenses/by-sa/4.0/" target="_blank" rel="noopener noreferrer">CC-BY-SA 4.0</a>. Documentation licensed under CC-BY-SA 4.0.`,
        },
        prism: {
            theme: prismThemes.github,
            darkTheme: prismThemes.dracula,
            additionalLanguages: ['rust', 'toml', 'bash', 'python', 'c'],
        },
    } satisfies Preset.ThemeConfig,
};

export default config;
