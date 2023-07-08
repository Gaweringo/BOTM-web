UPDATE users  SET expiry_timestamp = 'epoch' WHERE expiry_timestamp = '-infinity';
