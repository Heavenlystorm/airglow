# Community effects

Lighting effects made in Airglow's Studio, which every Airglow can browse, preview on its own case, and install
(Studio, Community effects). Each one is a single HTML file: `effects/<your-github-name>/<effect-name>.html`.

## Sharing yours
In Airglow, open the Studio, make or open your effect, and press **Share**. GitHub opens in your browser with the
file filled in (or, for a long one, with your effect on the clipboard to paste). Press **Propose changes**, then
**Create pull request**. That's it.

What happens next:
1. A check runs on your pull request: the file is valid, it's in your own folder, and it runs for 10 seconds without
   errors or slow frames. The run's summary says what it found.
2. A maintainer looks at it: does it do what it says, nothing hidden, and it fits the content rules below.
3. Once it's merged, it's in everyone's gallery within about 10 minutes.

To update your effect later, share it again (or edit the file in a pull request). You can only change files in your
own folder.

## The rules
- Your own work, or a remix of something open (the file says what it remixes, and the original is credited).
- An OSI-approved licence. The Studio uses MIT unless you pick another.
- No hidden behaviour. Effects run in a sandbox with no network, files or anything of Airglow's anyway; code that
  tries is turned down.
- Nothing hateful, sexual or meant to cause seizures (no full-brightness strobing faster than 3 times a second).
- Names and descriptions in plain words, and no trademarks you don't own in the name.

## For reviewers
The format and limits are in [`airglow/fxformat.py`](../airglow/fxformat.py) and
[docs/EFFECT-STUDIO.md](../docs/EFFECT-STUDIO.md). `featured.json` lists the effects shown first; `revoked.json` turns
effects off on every PC at its next gallery refresh, for example:
`[{"id": "jane-dev.aurora-wave", "sha256": ["<the bad version's hash>"], "reason": "Flashes faster than allowed"}]`
(leave `sha256` empty to revoke every version). `index.json` is rebuilt by a workflow after every merge; don't edit
it by hand.
