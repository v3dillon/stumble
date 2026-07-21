# Expire and withdraw public Pod Announcements

Each public Pod Announcement carries a renewable 30-day Announcement Lease, refreshed by its Origin Node periodically and when public metadata changes, so abandoned Pods do not remain discoverable forever. Peers stop relaying and considering expired announcements, while an Origin-signed Pod Withdrawal ends new discovery immediately; neither mechanism deletes existing Subscriptions or previously synchronized content.
