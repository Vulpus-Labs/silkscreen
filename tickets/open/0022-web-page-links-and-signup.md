---
id: "0022"
title: The web demo page carries links and an optional mailing-list form
priority: medium
created: 2026-11-27
epic: E002
depends: []
---

## Summary

After the splash, `mountSynthPage`'s page is the faceplate, the keyboard
and a CPU meter: nothing leads back to the synth's site or lets a player
who has enjoyed it hear about its release. Add a small bar of links and an
optional mailing-list form.

## Acceptance criteria

- [ ] `mountSynthPage({ links })`, an array of `{ text, href }`, shows the
      links in a bar in the top right, each opening in a new tab
- [ ] `mountSynthPage({ signup })`, `{ action, label?, text?, fields? }`,
      adds a button to the bar that opens a small form: one email field,
      posted to `action` with `fields` as hidden inputs, in a new tab,
      so the player keeps playing
- [ ] typing in the email field plays no notes and Escape closes the form
- [ ] without either option the page is unchanged
- [ ] design/hosts.md documents both options

## Notes

The form posts a plain HTML form; nothing about a particular mailing-list
service is built in. Buttondown, for example, takes
`action: "https://buttondown.com/api/emails/embed-subscribe/<list>"` and a
`tag` field.
