//! A sheet sketch can save a SymmetricLines constraint that mirrors supporting lines about an
//! axis. Older drawings keep their legacy Symmetric point constraints and their mirrored endpoints.
//! The previous release warns about the newer format and may refuse a standalone drawing if it
//! cannot read its sheet sketches. A design container keeps the unread drawing when saved.
