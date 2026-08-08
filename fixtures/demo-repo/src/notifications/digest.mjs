export function shouldSendDigest(preferences) {
  return preferences.emailEnabled && preferences.digestFrequency !== "never";
}
