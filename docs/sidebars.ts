import type { SidebarsConfig } from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
    docsSidebar: [
        'intro',
        {
            type: 'category',
            label: 'Use the dictionary',
            collapsed: false,
            items: [
                'install',
                'quickstart-cli',
                'quickstart-rust',
                'quickstart-python',
                'quickstart-c',
                'python-api',
            ],
        },
        {
            type: 'category',
            label: 'Format',
            items: [
                'format',
                'private-tags',
            ],
        },
        {
            type: 'category',
            label: 'Reference',
            items: [
                'data-sources',
                'roadmap',
            ],
        },
    ],
};

export default sidebars;
