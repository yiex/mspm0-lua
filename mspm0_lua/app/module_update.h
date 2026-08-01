#ifndef MODULE_UPDATE_H
#define MODULE_UPDATE_H

/* Transactional native-module updates stored in external LittleFS. */
int module_update_has_pending(void);
int module_update_stage(const char *path);
int module_update_apply_pending(void);
void module_update_report_status(void);
const char *module_update_error(void);

#endif
