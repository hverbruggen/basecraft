.pragma library
// Settings forms as the alignment controller returns them, copied from the
// plugin manifests (plugins/*/plugin.toml).
var forms = {
 "iqtree/tree": {
  "id": "tree",
  "title": "IQ-TREE",
  "operation": "tree",
  "button": "Build tree",
  "output": [
   "iqtree",
   "nwk"
  ],
  "min_rows": 4,
  "working": "Building a tree with IQ-TREE…",
  "threads": true,
  "note": "ModelFinder tests models with and without rate variation and uses the best by BIC. Uses the allocated CPU threads. Support values are written on the branches of the tree.",
  "fields": [
   {
    "name": "model",
    "label": "Substitution model",
    "type": "choice",
    "default": "MFP",
    "choices": [
     [
      "MFP",
      "ModelFinder (best fit by BIC)"
     ],
     [
      "JC",
      "JC (DNA)"
     ],
     [
      "F81",
      "F81 (DNA)"
     ],
     [
      "K2P",
      "K2P (DNA)"
     ],
     [
      "HKY",
      "HKY (DNA)"
     ],
     [
      "TN",
      "TN (DNA)"
     ],
     [
      "TIM",
      "TIM (DNA)"
     ],
     [
      "TVM",
      "TVM (DNA)"
     ],
     [
      "GTR",
      "GTR (DNA)"
     ],
     [
      "LG",
      "LG (protein)"
     ],
     [
      "WAG",
      "WAG (protein)"
     ],
     [
      "JTT",
      "JTT (protein)"
     ],
     [
      "Dayhoff",
      "Dayhoff (protein)"
     ],
     [
      "mtREV",
      "mtREV (protein)"
     ],
     [
      "cpREV",
      "cpREV (protein)"
     ],
     [
      "Blosum62",
      "Blosum62 (protein)"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "gamma",
    "label": "Gamma rate variation (+G)",
    "type": "boolean",
    "default": true,
    "when": "model != MFP",
    "choices": [],
    "min": null,
    "max": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "invariant",
    "label": "Invariant sites (+I)",
    "type": "boolean",
    "default": false,
    "when": "model != MFP",
    "choices": [],
    "min": null,
    "max": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "support",
    "label": "Branch support",
    "type": "choice",
    "default": "none",
    "choices": [
     [
      "none",
      "None"
     ],
     [
      "ufboot",
      "Ultrafast bootstrap (UFBoot)"
     ],
     [
      "bootstrap",
      "Standard bootstrap"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "replicates",
    "label": "Replicates",
    "type": "integer",
    "default": 1000,
    "min": 1,
    "max": 100000,
    "when": "support != none",
    "min_when": [
     [
      "support == ufboot",
      1000
     ]
    ],
    "choices": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "seed",
    "label": "Seed",
    "type": "integer",
    "default": 1,
    "min": 1,
    "max": 100000,
    "choices": [],
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   }
  ],
  "fixed": {}
 },
 "fasttree/tree": {
  "id": "tree",
  "title": "FastTree",
  "operation": "tree",
  "button": "Build tree",
  "output": [
   "fasttree",
   "nwk"
  ],
  "min_rows": 3,
  "working": "Building a tree with FastTree…",
  "note": "SH-like local support is a fast test of each branch, written on the tree as a value from 0 to 1. It is not a bootstrap proportion. Identical sequences are joined without a support value.",
  "fields": [
   {
    "name": "model",
    "label": "Substitution model",
    "type": "choice",
    "default": "",
    "choices": [
     [
      "",
      "GTR for DNA, LG for protein (standard)"
     ],
     [
      "JC",
      "JC (DNA)"
     ],
     [
      "GTR",
      "GTR (DNA)"
     ],
     [
      "LG",
      "LG (protein)"
     ],
     [
      "WAG",
      "WAG (protein)"
     ],
     [
      "JTT",
      "JTT (protein)"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "gamma",
    "label": "Gamma rate variation (rescales lengths; default: CAT categories)",
    "type": "boolean",
    "default": false,
    "choices": [],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "support",
    "label": "SH-like local support",
    "type": "boolean",
    "default": true,
    "choices": [],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "resamples",
    "label": "Resamples",
    "type": "integer",
    "default": 1000,
    "min": 1,
    "max": 100000,
    "when": "support == true",
    "choices": [],
    "min_when": [],
    "value_when": [],
    "local": false
   }
  ],
  "fixed": {},
  "threads": false
 },
 "distance/matrix": {
  "id": "matrix",
  "title": "Distance matrix",
  "operation": "matrix",
  "button": "Calculate",
  "output": [
   "distances",
   "tsv"
  ],
  "min_rows": 2,
  "working": "Calculating distances…",
  "note": "Gaps and ambiguous residues are excluded pairwise.",
  "fields": [
   {
    "name": "model",
    "label": "DNA model",
    "type": "choice",
    "default": "p",
    "choices": [
     [
      "p",
      "p"
     ],
     [
      "jc69",
      "jc69"
     ],
     [
      "k2p",
      "k2p"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "min_overlap",
    "label": "Minimum shared sites",
    "type": "integer",
    "default": 1,
    "min": 1,
    "max": 100000000,
    "choices": [],
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   }
  ],
  "fixed": {},
  "threads": false
 },
 "distance/nj": {
  "id": "nj",
  "title": "Neighbor joining",
  "operation": "tree",
  "button": "Build tree",
  "output": [
   "nj",
   "nwk"
  ],
  "min_rows": 2,
  "working": "Calculating distances…",
  "note": "Bootstrap uses complete distances; any failed replicate withholds support. Gaps and ambiguous residues are excluded pairwise.",
  "fields": [
   {
    "name": "model",
    "label": "DNA model",
    "type": "choice",
    "default": "k2p",
    "choices": [
     [
      "p",
      "p"
     ],
     [
      "jc69",
      "jc69"
     ],
     [
      "k2p",
      "k2p"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "min_overlap",
    "label": "Minimum shared sites",
    "type": "integer",
    "default": 1,
    "min": 1,
    "max": 100000000,
    "choices": [],
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "method",
    "label": "Missing-distance method",
    "type": "choice",
    "default": "njstar",
    "choices": [
     [
      "njstar",
      "njstar"
     ],
     [
      "sorted",
      "sorted"
     ],
     [
      "fourpoint",
      "fourpoint"
     ],
     [
      "mvrstar",
      "mvrstar"
     ]
    ],
    "value_when": [
     [
      "bootstrap_on == true",
      "sorted"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "local": false
   },
   {
    "name": "bootstrap_on",
    "label": "Bootstrap",
    "type": "boolean",
    "default": false,
    "local": true,
    "choices": [],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": []
   },
   {
    "name": "bootstrap",
    "label": "Replicates",
    "type": "integer",
    "default": 1000,
    "min": 1,
    "max": 1000000,
    "when": "bootstrap_on == true",
    "choices": [],
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "seed",
    "label": "Seed",
    "type": "integer",
    "default": 1,
    "min": 0,
    "max": 2147483647,
    "when": "bootstrap_on == true",
    "choices": [],
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "bootstrap_destination",
    "label": "Save bootstrap trees in",
    "type": "choice",
    "default": "metadata",
    "choices": [
     [
      "metadata",
      "Metadata directory (.basecraft/)"
     ],
     [
      "working",
      "Working directory"
     ]
    ],
    "when": "bootstrap_on == true",
    "min": null,
    "max": null,
    "min_when": [],
    "value_when": [],
    "local": false
   }
  ],
  "fixed": {},
  "threads": false
 },
 "distance/upgma": {
  "id": "upgma",
  "title": "UPGMA",
  "operation": "tree",
  "button": "Build tree",
  "output": [
   "upgma",
   "nwk"
  ],
  "min_rows": 2,
  "working": "Calculating distances…",
  "note": "UPGMA requires complete distances within each connected group. Bootstrap uses complete distances; any failed replicate withholds support. Gaps and ambiguous residues are excluded pairwise.",
  "fixed": {
   "method": "upgma"
  },
  "fields": [
   {
    "name": "model",
    "label": "DNA model",
    "type": "choice",
    "default": "k2p",
    "choices": [
     [
      "p",
      "p"
     ],
     [
      "jc69",
      "jc69"
     ],
     [
      "k2p",
      "k2p"
     ]
    ],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "min_overlap",
    "label": "Minimum shared sites",
    "type": "integer",
    "default": 1,
    "min": 1,
    "max": 100000000,
    "choices": [],
    "when": null,
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "bootstrap_on",
    "label": "Bootstrap",
    "type": "boolean",
    "default": false,
    "local": true,
    "choices": [],
    "min": null,
    "max": null,
    "when": null,
    "min_when": [],
    "value_when": []
   },
   {
    "name": "bootstrap",
    "label": "Replicates",
    "type": "integer",
    "default": 1000,
    "min": 1,
    "max": 1000000,
    "when": "bootstrap_on == true",
    "choices": [],
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "seed",
    "label": "Seed",
    "type": "integer",
    "default": 1,
    "min": 0,
    "max": 2147483647,
    "when": "bootstrap_on == true",
    "choices": [],
    "min_when": [],
    "value_when": [],
    "local": false
   },
   {
    "name": "bootstrap_destination",
    "label": "Save bootstrap trees in",
    "type": "choice",
    "default": "metadata",
    "choices": [
     [
      "metadata",
      "Metadata directory (.basecraft/)"
     ],
     [
      "working",
      "Working directory"
     ]
    ],
    "when": "bootstrap_on == true",
    "min": null,
    "max": null,
    "min_when": [],
    "value_when": [],
    "local": false
   }
  ],
  "threads": false
 }
}

function target() {
    return {
        busy: false,
        message: "",
        message_kind: "",
        runs: [],
        tool_form: function (plugin, id) {
            const form = forms[plugin + "/" + id]
            return JSON.stringify(form || {error: plugin + " has no " + id + " settings"})
        },
        run_form: function (plugin, id, values) { this.runs.push({plugin: plugin, form: id, values: JSON.parse(values)}) }
    }
}
